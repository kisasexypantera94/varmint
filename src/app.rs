use crate::{
    cpu::SystemRequest,
    devices::{RuntimeEvent, RuntimeInputEvent},
    display::{DisplayBuffer, PresentMode, Presenter},
    macos_ui,
    virtio::input::keys::*,
};
use std::{
    collections::BTreeSet,
    sync::{Mutex, mpsc::Sender},
    time::{Duration, Instant},
};

const PRESENT_KEEPALIVE: Duration = Duration::from_millis(250);
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{DeviceEvent, ElementState, Modifiers, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::PhysicalKey,
    platform::macos::ActiveEventLoopExtMacOS,
    window::{CursorGrabMode, Fullscreen, Window, WindowId},
};

#[derive(Clone, Copy)]
pub enum AppEvent {
    DisplayChanged,
    System(SystemRequest),
}

fn display_refresh_millihertz(window: &Window) -> u32 {
    window
        .current_monitor()
        .and_then(|monitor| monitor.refresh_rate_millihertz())
        .unwrap_or(60_000)
}

fn winit_to_linux_key(key: winit::keyboard::KeyCode) -> Option<u16> {
    use winit::keyboard::KeyCode::*;
    Some(match key {
        Escape => KEY_ESC,
        Digit1 => KEY_1,
        Digit2 => KEY_2,
        Digit3 => KEY_3,
        Digit4 => KEY_4,
        Digit5 => KEY_5,
        Digit6 => KEY_6,
        Digit7 => KEY_7,
        Digit8 => KEY_8,
        Digit9 => KEY_9,
        Digit0 => KEY_0,
        Minus => KEY_MINUS,
        Equal => KEY_EQUAL,
        Backspace => KEY_BACKSPACE,
        Tab => KEY_TAB,
        KeyQ => KEY_Q,
        KeyW => KEY_W,
        KeyE => KEY_E,
        KeyR => KEY_R,
        KeyT => KEY_T,
        KeyY => KEY_Y,
        KeyU => KEY_U,
        KeyI => KEY_I,
        KeyO => KEY_O,
        KeyP => KEY_P,
        BracketLeft => KEY_LEFTBRACE,
        BracketRight => KEY_RIGHTBRACE,
        Enter => KEY_ENTER,
        ControlLeft => KEY_LEFTCTRL,
        KeyA => KEY_A,
        KeyS => KEY_S,
        KeyD => KEY_D,
        KeyF => KEY_F,
        KeyG => KEY_G,
        KeyH => KEY_H,
        KeyJ => KEY_J,
        KeyK => KEY_K,
        KeyL => KEY_L,
        Semicolon => KEY_SEMICOLON,
        Quote => KEY_APOSTROPHE,
        Backquote => KEY_GRAVE,
        ShiftLeft => KEY_LEFTSHIFT,
        Backslash => KEY_BACKSLASH,
        KeyZ => KEY_Z,
        KeyX => KEY_X,
        KeyC => KEY_C,
        KeyV => KEY_V,
        KeyB => KEY_B,
        KeyN => KEY_N,
        KeyM => KEY_M,
        Comma => KEY_COMMA,
        Period => KEY_DOT,
        Slash => KEY_SLASH,
        ShiftRight => KEY_RIGHTSHIFT,
        AltLeft => KEY_LEFTALT,
        Space => KEY_SPACE,
        CapsLock => KEY_CAPSLOCK,
        F1 => KEY_F1,
        F2 => KEY_F2,
        F3 => KEY_F3,
        F4 => KEY_F4,
        F5 => KEY_F5,
        F6 => KEY_F6,
        F7 => KEY_F7,
        F8 => KEY_F8,
        F9 => KEY_F9,
        F10 => KEY_F10,
        F11 => KEY_F11,
        F12 => KEY_F12,
        NumLock => KEY_NUMLOCK,
        ScrollLock => KEY_SCROLLLOCK,
        Numpad7 => KEY_KP7,
        Numpad8 => KEY_KP8,
        Numpad9 => KEY_KP9,
        NumpadSubtract => KEY_KPMINUS,
        Numpad4 => KEY_KP4,
        Numpad5 => KEY_KP5,
        Numpad6 => KEY_KP6,
        NumpadAdd => KEY_KPPLUS,
        Numpad1 => KEY_KP1,
        Numpad2 => KEY_KP2,
        Numpad3 => KEY_KP3,
        Numpad0 => KEY_KP0,
        NumpadDecimal => KEY_KPDOT,
        NumpadEnter => KEY_KPENTER,
        NumpadDivide => KEY_KPSLASH,
        NumpadMultiply => KEY_KPASTERISK,
        ControlRight => KEY_RIGHTCTRL,
        AltRight => KEY_RIGHTALT,
        Home => KEY_HOME,
        ArrowUp => KEY_UP,
        PageUp => KEY_PAGEUP,
        ArrowLeft => KEY_LEFT,
        ArrowRight => KEY_RIGHT,
        End => KEY_END,
        ArrowDown => KEY_DOWN,
        PageDown => KEY_PAGEDOWN,
        Insert => KEY_INSERT,
        Delete => KEY_DELETE,
        _ => return None,
    })
}

struct AppState<'a> {
    display: &'a Mutex<DisplayBuffer>,
    host_tx: Sender<RuntimeEvent>,
    on_exit: Option<Box<dyn FnOnce() + 'a>>,

    presenter: Option<Presenter>,

    surface_w: u32,
    surface_h: u32,

    front: Vec<u32>,
    present_w: usize,
    present_h: usize,
    dirty_rect: Option<(usize, usize, usize, usize)>,
    iosurface_id: Option<u32>,
    last_seq: u64,
    pending_display_updates: u64,
    frame_pending: bool,
    present_interval: Duration,
    next_present: Instant,
    next_keepalive: Instant,

    last_mouse_pos: Option<(f64, f64)>,
    mouse_captured: bool,
    capture_chord_held: bool,
    pressed_keys: BTreeSet<u16>,
    rel_mouse_frac_dx: f64,
    rel_mouse_frac_dy: f64,

    stats_visible: bool,

    shutdown_requested: bool,
    restart: bool,
}

impl<'a> AppState<'a> {
    fn new(display: &'a Mutex<DisplayBuffer>, host_tx: Sender<RuntimeEvent>, on_exit: Box<dyn FnOnce() + 'a>) -> Self {
        Self {
            display,
            host_tx,
            on_exit: Some(on_exit),
            presenter: None,
            surface_w: 0,
            surface_h: 0,
            front: Vec::new(),
            present_w: 0,
            present_h: 0,
            dirty_rect: None,
            iosurface_id: None,
            last_seq: 0,
            pending_display_updates: 0,
            frame_pending: false,
            present_interval: Duration::from_nanos(16_666_667),
            next_present: Instant::now(),
            next_keepalive: Instant::now(),
            last_mouse_pos: None,
            mouse_captured: false,
            capture_chord_held: false,
            pressed_keys: BTreeSet::new(),
            rel_mouse_frac_dx: 0.0,
            rel_mouse_frac_dy: 0.0,
            stats_visible: false,
            shutdown_requested: false,
            restart: false,
        }
    }

    fn present(&mut self, requested_mode: PresentMode) -> bool {
        self.next_keepalive = Instant::now() + PRESENT_KEEPALIVE;

        let mode = if self.frame_pending {
            PresentMode::NewFrame
        } else {
            requested_mode
        };

        let Some(presenter) = self.presenter.as_mut() else {
            return false;
        };
        if self.present_w == 0 || self.present_h == 0 {
            return false;
        }

        let full = (0, 0, self.present_w, self.present_h);
        let (x, y, width, height) = self.dirty_rect.unwrap_or(full);
        let display_updates = if mode == PresentMode::NewFrame {
            self.pending_display_updates
        } else {
            0
        };
        let presented = presenter.present_iosurface_or_rect(
            self.iosurface_id,
            &self.front,
            self.present_w as u32,
            self.present_h as u32,
            x as u32,
            y as u32,
            width as u32,
            height as u32,
            mode,
            display_updates,
        );

        if presented && mode == PresentMode::NewFrame {
            self.dirty_rect = None;
            self.frame_pending = false;
            self.pending_display_updates = 0;
        }

        presented
    }

    fn set_present_refresh_millihertz(&mut self, refresh_millihertz: u32) {
        let refresh_millihertz = u64::from(refresh_millihertz.max(1));
        self.present_interval = Duration::from_nanos(1_000_000_000_000u64 / refresh_millihertz);
    }

    fn present_pending_if_due(&mut self) {
        if !self.frame_pending {
            return;
        }

        let now = Instant::now();
        if now < self.next_present {
            return;
        }

        self.present(PresentMode::NewFrame);
        self.next_present += self.present_interval;

        let now = Instant::now();
        if self.next_present <= now {
            self.next_present = now + self.present_interval;
        }
    }

    fn poll_display(&mut self) -> bool {
        let Some(update) = self.display.lock().unwrap().take_update(self.last_seq, &mut self.front) else {
            return false;
        };

        self.last_seq = update.sequence;
        self.pending_display_updates = self.pending_display_updates.saturating_add(update.update_count);
        if let Some(presenter) = self.presenter.as_mut() {
            presenter.record_display_updates(update.update_count);
            presenter.record_angle_copies(update.angle_copy);
        }
        self.present_w = update.width;
        self.present_h = update.height;
        self.iosurface_id = update.iosurface_id;
        self.frame_pending = true;

        let (x, y, w, h) = update.dirty_rect;
        if w != 0 && h != 0 {
            self.dirty_rect = Some(match self.dirty_rect {
                Some((old_x, old_y, old_w, old_h)) => {
                    let x0 = old_x.min(x);
                    let y0 = old_y.min(y);
                    let x1 = old_x.saturating_add(old_w).max(x.saturating_add(w));
                    let y1 = old_y.saturating_add(old_h).max(y.saturating_add(h));
                    (x0, y0, x1 - x0, y1 - y0)
                }
                None => (x, y, w, h),
            });
        }

        true
    }

    fn set_mouse_capture(&mut self, captured: bool) {
        if self.mouse_captured == captured {
            return;
        }

        let Some(presenter) = self.presenter.as_ref() else {
            return;
        };

        let window = presenter.window();
        let result = if captured {
            window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
        } else {
            window.set_cursor_grab(CursorGrabMode::None)
        };

        match result {
            Ok(()) => {
                self.mouse_captured = captured;
                self.rel_mouse_frac_dx = 0.0;
                self.rel_mouse_frac_dy = 0.0;
                window.set_cursor_visible(!captured);
                eprintln!(
                    "input: relative mouse capture {}",
                    if captured { "enabled" } else { "disabled" }
                );
            }
            Err(e) => eprintln!("input: failed to change mouse capture: {e}"),
        }
    }

    fn request_shutdown(&mut self, event_loop: &ActiveEventLoop) {
        if self.shutdown_requested {
            eprintln!("vm: quitting without waiting for guest shutdown");
            event_loop.exit();
            return;
        }

        eprintln!("vm: requesting guest shutdown");
        self.shutdown_requested = true;
        self.release_pressed_keys();
        self.set_mouse_capture(false);
        for pressed in [true, false] {
            let _ = self.host_tx.send(RuntimeEvent::Input(RuntimeInputEvent::Key {
                code: KEY_POWER,
                pressed,
            }));
        }
    }

    /// Ctrl+Command toggles capture. Command is never forwarded, so the chord can't trigger a
    /// guest shortcut.
    fn update_capture_chord(&mut self, modifiers: Modifiers) {
        let state = modifiers.state();
        let held = state.control_key() && state.super_key();
        if held && !self.capture_chord_held {
            self.release_pressed_keys();
            self.set_mouse_capture(!self.mouse_captured);
        }
        self.capture_chord_held = held;
    }

    fn update_key(&mut self, code: u16, pressed: bool) {
        let changed = if pressed {
            self.pressed_keys.insert(code)
        } else {
            self.pressed_keys.remove(&code)
        };
        if changed {
            let _ = self
                .host_tx
                .send(RuntimeEvent::Input(RuntimeInputEvent::Key { code, pressed }));
        }
    }

    fn release_pressed_keys(&mut self) {
        for code in std::mem::take(&mut self.pressed_keys) {
            let _ = self
                .host_tx
                .send(RuntimeEvent::Input(RuntimeInputEvent::Key { code, pressed: false }));
        }
    }

    fn take_relative_mouse_delta(accum: &mut f64, delta: f64) -> i32 {
        *accum += delta;
        let whole = if *accum >= 0.0 {
            (*accum).floor()
        } else {
            (*accum).ceil()
        };
        *accum -= whole;
        whole.clamp(i32::MIN as f64, i32::MAX as f64) as i32
    }
}

impl<'a> ApplicationHandler<AppEvent> for AppState<'a> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_allows_automatic_window_tabbing(false);

        let attrs = Window::default_attributes()
            .with_title("Varmint")
            .with_inner_size(LogicalSize::new(1024u32, 768u32))
            .with_fullscreen(Some(Fullscreen::Borderless(None)))
            .with_resizable(true);

        let window = event_loop.create_window(attrs).unwrap();
        let PhysicalSize { width, height } = window.inner_size();
        let logical_size = PhysicalSize::new(width, height).to_logical::<u32>(window.scale_factor());
        self.surface_w = width;
        self.surface_h = height;
        self.presenter = Some(Presenter::new(window, width, height));
        macos_ui::install_main_menu();
        macos_ui::set_statistics_checked(self.stats_visible);
        if let Some(presenter) = self.presenter.as_mut() {
            presenter.set_stats_visible(self.stats_visible);
        }

        let refresh_millihertz = display_refresh_millihertz(self.presenter.as_ref().unwrap().window());
        self.set_present_refresh_millihertz(refresh_millihertz);
        self.next_present = Instant::now();

        let _ = self.host_tx.send(RuntimeEvent::DisplayResized {
            physical_width: width,
            physical_height: height,
            logical_width: logical_size.width,
            logical_height: logical_size.height,
            refresh_millihertz,
        });

        self.poll_display();
        self.present_pending_if_due();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.request_shutdown(event_loop);
            }

            WindowEvent::Resized(PhysicalSize { width, height }) => {
                self.surface_w = width;
                self.surface_h = height;
                self.last_mouse_pos = None;

                let scale_factor = self
                    .presenter
                    .as_ref()
                    .map(|presenter| presenter.window().scale_factor())
                    .unwrap_or(1.0);

                if let Some(p) = self.presenter.as_mut() {
                    p.resize_surface(width, height);
                }

                let logical_size = PhysicalSize::new(width, height).to_logical::<u32>(scale_factor);
                let refresh_millihertz = display_refresh_millihertz(self.presenter.as_ref().unwrap().window());
                self.set_present_refresh_millihertz(refresh_millihertz);

                let _ = self.host_tx.send(RuntimeEvent::DisplayResized {
                    physical_width: width,
                    physical_height: height,
                    logical_width: logical_size.width,
                    logical_height: logical_size.height,
                    refresh_millihertz,
                });

                self.present(PresentMode::Redraw);
            }

            WindowEvent::RedrawRequested => {
                if self.frame_pending {
                    self.present_pending_if_due();
                } else {
                    self.present(PresentMode::Redraw);
                }
            }

            WindowEvent::Focused(false) => {
                self.release_pressed_keys();
                self.set_mouse_capture(false);
                self.capture_chord_held = false;
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                self.update_capture_chord(modifiers);
            }

            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(linux_code) = winit_to_linux_key(code) {
                        if self.capture_chord_held && matches!(linux_code, KEY_LEFTCTRL | KEY_RIGHTCTRL) {
                            return;
                        }
                        self.update_key(linux_code, pressed);
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                if !self.mouse_captured {
                    let PhysicalPosition { x, y } = position;
                    let pos = Some((x, y));
                    if pos != self.last_mouse_pos {
                        self.last_mouse_pos = pos;
                        let _ = self.host_tx.send(RuntimeEvent::Input(RuntimeInputEvent::PointerMove {
                            x: x as u32,
                            y: y as u32,
                            width: self.surface_w,
                            height: self.surface_h,
                        }));
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let btn = match button {
                    MouseButton::Left => BTN_LEFT,
                    MouseButton::Right => BTN_RIGHT,
                    MouseButton::Middle => BTN_MIDDLE,
                    _ => return,
                };

                if !self.mouse_captured && state == ElementState::Pressed {
                    self.set_mouse_capture(true);
                    if self.mouse_captured {
                        return;
                    }
                }

                let _ = self.host_tx.send(RuntimeEvent::Input(RuntimeInputEvent::PointerButton {
                    button: btn,
                    pressed: state == ElementState::Pressed,
                    relative: self.mouse_captured,
                }));
            }

            WindowEvent::MouseWheel { delta, .. } => {
                use winit::event::MouseScrollDelta;
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32 / 32.0, p.y as f32 / 32.0),
                };
                if dy != 0.0 {
                    let _ = self.host_tx.send(RuntimeEvent::Input(RuntimeInputEvent::Scroll {
                        horizontal: false,
                        value: dy.round() as i32,
                        relative: self.mouse_captured,
                    }));
                }
                if dx != 0.0 {
                    let _ = self.host_tx.send(RuntimeEvent::Input(RuntimeInputEvent::Scroll {
                        horizontal: true,
                        value: dx.round() as i32,
                        relative: self.mouse_captured,
                    }));
                }
            }

            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::DisplayChanged => {
                self.poll_display();
                self.present_pending_if_due();
            }
            AppEvent::System(SystemRequest::PowerOff) => {
                eprintln!("vm: guest powered off");
                event_loop.exit();
            }
            AppEvent::System(SystemRequest::Reset) => {
                eprintln!("vm: guest reset");
                self.restart = true;
                event_loop.exit();
            }
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: winit::event::DeviceId, event: DeviceEvent) {
        if !self.mouse_captured {
            return;
        }

        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            let dx = Self::take_relative_mouse_delta(&mut self.rel_mouse_frac_dx, dx);
            let dy = Self::take_relative_mouse_delta(&mut self.rel_mouse_frac_dy, dy);
            if dx != 0 || dy != 0 {
                let _ = self
                    .host_tx
                    .send(RuntimeEvent::Input(RuntimeInputEvent::RelativeMouseMotion { dx, dy }));
            }
        }
    }

    /// Also runs for NSApp terminate: (Dock Quit, logout), which exits the process without returning from `run`.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(on_exit) = self.on_exit.take() {
            on_exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if macos_ui::take_toggle_statistics() {
            self.stats_visible = !self.stats_visible;
            macos_ui::set_statistics_checked(self.stats_visible);
            if let Some(presenter) = self.presenter.as_mut() {
                presenter.set_stats_visible(self.stats_visible);
            }
            self.present(PresentMode::Redraw);
        }
        if macos_ui::take_show_about() {
            macos_ui::show_about();
        }
        if macos_ui::take_quit() {
            self.request_shutdown(event_loop);
        }

        self.present_pending_if_due();

        if !self.frame_pending && Instant::now() >= self.next_keepalive {
            self.present(PresentMode::Redraw);
        }

        if let Some(presenter) = self.presenter.as_mut() {
            presenter.refresh_stats();
        }

        let deadline = if self.frame_pending {
            self.next_present
        } else {
            self.next_keepalive
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
    }
}

pub fn event_loop() -> EventLoop<AppEvent> {
    EventLoop::<AppEvent>::with_user_event().build().unwrap()
}

/// Returns true when the guest asked for a reset.
pub fn run<'a>(
    event_loop: EventLoop<AppEvent>,
    display: &'a Mutex<DisplayBuffer>,
    host_tx: Sender<RuntimeEvent>,
    on_exit: impl FnOnce() + 'a,
) -> bool {
    let mut app = AppState::new(display, host_tx, Box::new(on_exit));
    event_loop.run_app(&mut app).unwrap();
    app.restart
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, channel};

    fn receive_key(receiver: &Receiver<RuntimeEvent>) -> (u16, bool) {
        match receiver.recv().unwrap() {
            RuntimeEvent::Input(RuntimeInputEvent::Key { code, pressed }) => (code, pressed),
            _ => panic!("expected keyboard event"),
        }
    }

    #[test]
    fn releasing_pressed_keys_clears_modifiers_and_suppresses_late_releases() {
        let display = Mutex::new(DisplayBuffer::new());
        let (sender, receiver) = channel();
        let mut app = AppState::new(&display, sender, Box::new(|| {}));

        app.update_key(KEY_LEFTCTRL, true);
        app.update_key(KEY_LEFTCTRL, true);
        app.update_key(KEY_LEFTSHIFT, true);
        app.release_pressed_keys();
        app.update_key(KEY_LEFTCTRL, false);
        app.update_key(KEY_LEFTSHIFT, false);

        assert_eq!(receive_key(&receiver), (KEY_LEFTCTRL, true));
        assert_eq!(receive_key(&receiver), (KEY_LEFTSHIFT, true));
        assert_eq!(receive_key(&receiver), (KEY_LEFTCTRL, false));
        assert_eq!(receive_key(&receiver), (KEY_LEFTSHIFT, false));
        assert!(receiver.try_recv().is_err());
    }
}
