use crate::{
    devices::{RuntimeEvent, RuntimeInputEvent},
    display::{DisplayBuffer, Presenter},
    virtio::input::keys::*,
};
use std::{
    sync::{Mutex, mpsc::Sender},
    time::{Duration, Instant},
};

const PRESENT_KEEPALIVE: Duration = Duration::from_millis(250);
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{DeviceEvent, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

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

    presenter: Option<Presenter>,

    surface_w: u32,
    surface_h: u32,

    front: Vec<u32>,
    present_w: usize,
    present_h: usize,
    dirty_rect: Option<(usize, usize, usize, usize)>,
    iosurface_id: Option<u32>,
    last_seq: u64,
    next_keepalive: Instant,

    last_mouse_pos: Option<(f64, f64)>,
    mouse_captured: bool,
    rel_mouse_frac_dx: f64,
    rel_mouse_frac_dy: f64,

    stat_last: std::time::Instant,
    stat_produced: u64,
    stat_presented: u64,
    stat_update_ns: u128,
    stat_loops: u64,
}

impl<'a> AppState<'a> {
    fn new(display: &'a Mutex<DisplayBuffer>, host_tx: Sender<RuntimeEvent>) -> Self {
        Self {
            display,
            host_tx,
            presenter: None,
            surface_w: 0,
            surface_h: 0,
            front: Vec::new(),
            present_w: 0,
            present_h: 0,
            dirty_rect: None,
            iosurface_id: None,
            last_seq: 0,
            next_keepalive: Instant::now(),
            last_mouse_pos: None,
            mouse_captured: false,
            rel_mouse_frac_dx: 0.0,
            rel_mouse_frac_dy: 0.0,
            stat_last: std::time::Instant::now(),
            stat_produced: 0,
            stat_presented: 0,
            stat_update_ns: 0,
            stat_loops: 0,
        }
    }

    fn blit(&mut self, cached: bool) -> bool {
        if cached {
            self.next_keepalive = Instant::now() + PRESENT_KEEPALIVE;
        }

        if let Some(p) = self.presenter.as_mut() {
            if self.present_w > 0 && self.present_h > 0 {
                let t0 = std::time::Instant::now();

                let full = (0, 0, self.present_w, self.present_h);
                let (x, y, w, h) = self.dirty_rect.unwrap_or(full);

                let presented = p.present_iosurface_or_rect(
                    self.iosurface_id,
                    &self.front,
                    self.present_w as u32,
                    self.present_h as u32,
                    x as u32,
                    y as u32,
                    w as u32,
                    h as u32,
                    cached,
                );

                self.stat_update_ns += t0.elapsed().as_nanos();
                if presented {
                    self.stat_presented += 1;
                    if !cached {
                        self.dirty_rect = None;
                    }
                    self.next_keepalive = Instant::now() + PRESENT_KEEPALIVE;
                }
                return presented;
            }
        }

        false
    }

    fn poll_display(&mut self) -> bool {
        let mut display = self.display.lock().unwrap();

        if display.seq == self.last_seq {
            return false;
        }

        self.stat_produced += display.seq.wrapping_sub(self.last_seq);
        self.last_seq = display.seq;

        self.present_w = display.width;
        self.present_h = display.height;
        self.iosurface_id = display.iosurface_id;
        let native_copy_frame = self.iosurface_id.is_some();

        let full_dirty = (0, 0, display.width, display.height);
        let mut dirty = display.dirty_rect.take().unwrap_or(full_dirty);

        let resized = self.front.len() != display.pixels.len();
        if resized {
            self.front.resize(display.pixels.len(), 0);
            dirty = full_dirty;
        }

        let (x, y, w, h) = dirty;
        let x = x.min(display.width);
        let y = y.min(display.height);
        let w = w.min(display.width.saturating_sub(x));
        let h = h.min(display.height.saturating_sub(y));

        if w != 0 && h != 0 {
            if !native_copy_frame {
                for row in 0..h {
                    let src_off = (y + row) * display.width + x;
                    let dst_off = (y + row) * self.present_w + x;
                    self.front[dst_off..dst_off + w].copy_from_slice(&display.pixels[src_off..src_off + w]);
                }
            }

            let new_dirty = (x, y, w, h);
            self.dirty_rect = match self.dirty_rect {
                Some((old_x, old_y, old_w, old_h)) => {
                    let old_x1 = old_x.saturating_add(old_w);
                    let old_y1 = old_y.saturating_add(old_h);
                    let new_x1 = x.saturating_add(w);
                    let new_y1 = y.saturating_add(h);
                    let nx0 = old_x.min(x);
                    let ny0 = old_y.min(y);
                    let nx1 = old_x1.max(new_x1);
                    let ny1 = old_y1.max(new_y1);
                    Some((nx0, ny0, nx1 - nx0, ny1 - ny0))
                }
                None => Some(new_dirty),
            };
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

    fn print_stats(&mut self) {
        if self.stat_last.elapsed() >= std::time::Duration::from_secs(1) {
            let avg_ms = if self.stat_presented == 0 {
                0.0
            } else {
                self.stat_update_ns as f64 / self.stat_presented as f64 / 1_000_000.0
            };
            eprintln!(
                "display: loops={} produced={} presented={} avg_update_ms={:.3}",
                self.stat_loops, self.stat_produced, self.stat_presented, avg_ms,
            );
            self.stat_loops = 0;
            self.stat_produced = 0;
            self.stat_presented = 0;
            self.stat_update_ns = 0;
            self.stat_last = std::time::Instant::now();
        }
    }
}

impl<'a> ApplicationHandler for AppState<'a> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("Varmint")
            .with_inner_size(LogicalSize::new(1024u32, 768u32))
            .with_resizable(true);

        let window = event_loop.create_window(attrs).unwrap();
        let PhysicalSize { width, height } = window.inner_size();
        let logical_size = PhysicalSize::new(width, height).to_logical::<u32>(window.scale_factor());
        self.surface_w = width;
        self.surface_h = height;
        self.presenter = Some(Presenter::new(window, width, height));

        let _ = self.host_tx.send(RuntimeEvent::DisplayResized {
            width: logical_size.width,
            height: logical_size.height,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
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
                let _ = self.host_tx.send(RuntimeEvent::DisplayResized {
                    width: logical_size.width,
                    height: logical_size.height,
                });

                self.blit(false);
            }

            WindowEvent::RedrawRequested => {
                self.blit(false);
            }

            WindowEvent::Focused(focused) => {
                self.set_mouse_capture(focused);
            }

            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    if code == KeyCode::F12 {
                        if pressed {
                            self.set_mouse_capture(!self.mouse_captured);
                        }
                        return;
                    }
                    if pressed && code == KeyCode::F11 {
                        self.set_mouse_capture(false);
                    }
                    if let Some(linux_code) = winit_to_linux_key(code) {
                        let _ = self.host_tx.send(RuntimeEvent::Input(RuntimeInputEvent::Key {
                            code: linux_code,
                            pressed,
                        }));
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

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let display_changed = self.poll_display();
        if display_changed || self.dirty_rect.is_some() {
            self.blit(false);
        } else if Instant::now() >= self.next_keepalive {
            self.blit(true);
        }

        self.stat_loops += 1;
        self.print_stats();
    }
}

pub fn run(display: &Mutex<DisplayBuffer>, host_tx: Sender<RuntimeEvent>) {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = AppState::new(display, host_tx);
    event_loop.run_app(&mut app).unwrap();
}
