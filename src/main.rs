use crate::{
    machine::*,
    virtio::{
        input::keys::*,
        virgl_ffi::{VirglFence, VirglRenderer},
    },
};
use applevisor::prelude::*;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::{
    fs::File,
    io::{self, Read, Write},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, SyncSender},
    },
    thread,
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{DeviceEvent, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

mod angle_egl;
mod audio;
mod clipboard;
mod helpers;
mod iosurface;
mod irq;
mod kick;
mod linux;
mod machine;
mod memory;
mod net;
mod present;
mod sys_reg;
mod uart;
mod virtio;

fn read_file(path: &str) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> std::io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

fn stdin_thread(kicker: kick::Kicker, uart: &Mutex<uart::Uart>) {
    let _raw = RawModeGuard::new().unwrap();
    let stdin = std::io::stdin();
    let mut buf = [0u8; 1];

    const PREFIX: u8 = 0x1d; // Ctrl-]
    let mut got_prefix = false;

    eprintln!("[VM] Press Ctrl-] x to exit");

    loop {
        match stdin.lock().read(&mut buf) {
            Ok(0) => break,
            Ok(_) => {
                let b = buf[0];

                if got_prefix {
                    got_prefix = false;
                    match b {
                        b'x' => {
                            eprintln!("Received break command");
                            break;
                        }
                        _ => eprint!("unknown command: {b:#x}\r\n"),
                    }
                    continue;
                }

                if b == PREFIX {
                    got_prefix = true;
                    continue;
                }

                if uart.lock().unwrap().enqueue(b) {
                    kicker.kick();
                }
            }
            Err(e) => {
                eprintln!("stdin read error: {e}");
                break;
            }
        }
    }
}

fn mmio_read_reg(vcpu: &Vcpu, rt: u64) -> Result<u32> {
    Ok(match helpers::reg_from_rt(rt) {
        Some(reg) => vcpu.get_reg(reg)? as u32,
        None => 0,
    })
}

fn mmio_write_reg(vcpu: &Vcpu, rt: u64, value: u64) -> Result<()> {
    if let Some(reg) = helpers::reg_from_rt(rt) {
        vcpu.set_reg(reg, value)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct SecondaryStart {
    entry_point: u64,
    context_id: u64,
}

enum PsciAction {
    Continue,
    CpuOff,
}

fn handle_psci_hvc(
    vcpu: &Vcpu,
    vcpu_id: usize,
    secondary_boot_txs: &[SyncSender<SecondaryStart>],
    secondary_online: &[AtomicBool],
) -> Result<PsciAction> {
    let function_id = vcpu.get_reg(Reg::X0)?;

    let ret = match function_id {
        PSCI_VERSION => PSCI_VERSION_0_2,
        PSCI_CPU_ON_64 => {
            let target_mpidr = vcpu.get_reg(Reg::X1)?;
            let entry_point = vcpu.get_reg(Reg::X2)?;
            let context_id = vcpu.get_reg(Reg::X3)?;

            match secondary_index_for_mpidr(target_mpidr) {
                Some(index) => {
                    if secondary_online[index].swap(true, Ordering::SeqCst) {
                        PSCI_ALREADY_ON
                    } else {
                        let start = SecondaryStart {
                            entry_point,
                            context_id,
                        };
                        match secondary_boot_txs[index].send(start) {
                            Ok(()) => PSCI_SUCCESS,
                            Err(_) => {
                                secondary_online[index].store(false, Ordering::SeqCst);
                                PSCI_DENIED
                            }
                        }
                    }
                }
                None => PSCI_INVALID_PARAMETERS,
            }
        }
        PSCI_CPU_OFF => {
            if vcpu_id >= FIRST_SECONDARY_VCPU_ID {
                let index = vcpu_id - FIRST_SECONDARY_VCPU_ID;
                secondary_online[index].store(false, Ordering::SeqCst);
                vcpu.set_reg(Reg::X0, PSCI_SUCCESS)?;
                return Ok(PsciAction::CpuOff);
            }
            PSCI_DENIED
        }
        PSCI_SYSTEM_OFF | PSCI_SYSTEM_RESET => {
            eprintln!("PSCI system off/reset requested");
            PSCI_SUCCESS
        }
        _ => PSCI_NOT_SUPPORTED,
    };

    vcpu.set_reg(Reg::X0, ret)?;

    Ok(PsciAction::Continue)
}

enum DeviceThreadRequest {
    Mmio {
        is_write: bool,
        offset: u64,
        size: usize,
        value: u64,
        resp: SyncSender<Option<u64>>,
    },
    MmioWriteAsync {
        offset: u64,
        size: usize,
        value: u64,
    },
    GpuEvent(virtio::gpu::ExternalEvent),
}

fn send_gpu_mmio(
    gpu_tx: &Sender<DeviceThreadRequest>,
    offset: u64,
    size: usize,
    is_write: bool,
    value: u64,
) -> Option<u64> {
    if is_write && offset == VIRTIO_MMIO_QUEUE_NOTIFY {
        gpu_tx
            .send(DeviceThreadRequest::MmioWriteAsync { offset, size, value })
            .expect("gpu owner thread exited");
        return None;
    }

    let (resp_tx, resp_rx) = std::sync::mpsc::sync_channel(1024);
    gpu_tx
        .send(DeviceThreadRequest::Mmio {
            is_write,
            offset,
            size,
            value,
            resp: resp_tx,
        })
        .expect("gpu owner thread exited");
    resp_rx.recv().expect("gpu owner thread dropped MMIO response")
}

fn send_gpu_event(gpu_tx: &Sender<DeviceThreadRequest>, event: virtio::gpu::ExternalEvent) {
    let _ = gpu_tx.send(DeviceThreadRequest::GpuEvent(event));
}

fn gpu_owner_thread(
    mem: &memory::GuestMemory,
    display: &Mutex<virtio::gpu::DisplayBuffer>,
    rx: Receiver<DeviceThreadRequest>,
    irq_asserted: &AtomicBool,
    gpu_tx: Sender<DeviceThreadRequest>,
    kicker: kick::Kicker,
) {
    let mut renderer = build_virglrenderer(gpu_tx, kicker).expect("virglrenderer init failed");
    let gpu_dev = virtio::Gpu::new(display, &mut renderer);
    let mut gpu = virtio::MmioTransport::new(gpu_dev);

    let update_irq = |gpu: &mut virtio::MmioTransport<virtio::Gpu>| {
        irq_asserted.store(gpu.is_asserted(), Ordering::SeqCst);
    };

    update_irq(&mut gpu);

    while let Ok(req) = rx.recv() {
        match req {
            DeviceThreadRequest::Mmio {
                is_write,
                offset,
                size,
                value,
                resp,
            } => {
                let ret = if is_write {
                    gpu.write(offset, size, value, mem);
                    None
                } else {
                    Some(gpu.read(offset, size))
                };
                update_irq(&mut gpu);
                let _ = resp.send(ret);
            }
            DeviceThreadRequest::MmioWriteAsync { offset, size, value } => {
                gpu.write(offset, size, value, mem);
                update_irq(&mut gpu);
            }
            DeviceThreadRequest::GpuEvent(event) => {
                gpu.handle_external_event(event, mem);
                update_irq(&mut gpu);
            }
        }
    }
}

fn handle_virtio_mmio<D: virtio::Device>(
    dev: &Mutex<virtio::MmioTransport<D>>,
    offset: u64,
    size: usize,
    is_write: bool,
    value: u64,
    mem: &memory::GuestMemory,
) -> Option<u64> {
    let mut dev = dev.lock().unwrap();
    if is_write {
        dev.write(offset, size, value, mem);
        None
    } else {
        Some(dev.read(offset, size))
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_inline_mmio(
    route: MmioRoute,
    is_write: bool,
    size: usize,
    value: u64,
    mem: &memory::GuestMemory,
    devices: &VmDevices<'_>,
    net_out_tx: &Sender<Vec<u8>>,
    clipboard_out_tx: &Sender<Vec<u8>>,
) -> Option<u64> {
    match route.device {
        MmioDevice::Uart => {
            if is_write {
                devices.uart.lock().unwrap().write(route.offset, value as u32, |value| {
                    io::stdout().write_all(&[value as u8]).unwrap();
                    io::stdout().flush().unwrap();
                });
                None
            } else {
                Some(devices.uart.lock().unwrap().read(route.offset) as u64)
            }
        }
        MmioDevice::VirtioBlk => handle_virtio_mmio(devices.blk, route.offset, size, is_write, value, mem),
        MmioDevice::VirtioNet => {
            let ret = handle_virtio_mmio(devices.net, route.offset, size, is_write, value, mem);
            if is_write {
                while let Some(frame) = devices.net.lock().unwrap().pop_external() {
                    let _ = net_out_tx.send(frame);
                }
            }
            ret
        }
        MmioDevice::VirtioGpu => panic!("virtio-gpu is not an inline device"),
        MmioDevice::VirtioInputKeyboard => {
            handle_virtio_mmio(devices.keyboard, route.offset, size, is_write, value, mem)
        }
        MmioDevice::VirtioInputTablet => handle_virtio_mmio(devices.tablet, route.offset, size, is_write, value, mem),
        MmioDevice::VirtioInputMouse => handle_virtio_mmio(devices.mouse, route.offset, size, is_write, value, mem),
        MmioDevice::VirtioSnd => handle_virtio_mmio(devices.snd, route.offset, size, is_write, value, mem),
        MmioDevice::VirtioConsole => {
            let ret = handle_virtio_mmio(devices.console, route.offset, size, is_write, value, mem);
            if is_write {
                let mut virtio_console = devices.console.lock().unwrap();
                while let Some(bytes) = virtio_console.pop_external() {
                    let _ = clipboard_out_tx.send(bytes);
                }
                virtio_console.handle_external_event(virtio::console::ExternalEvent::RxAvailable, mem);
            }
            ret
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_routed_mmio(
    route: MmioRoute,
    is_write: bool,
    size: usize,
    value: u64,
    mem: &memory::GuestMemory,
    devices: &VmDevices<'_>,
    net_out_tx: &Sender<Vec<u8>>,
    clipboard_out_tx: &Sender<Vec<u8>>,
) -> Option<u64> {
    match route.placement {
        DevicePlacement::Inline => {
            handle_inline_mmio(route, is_write, size, value, mem, devices, net_out_tx, clipboard_out_tx)
        }
        DevicePlacement::ThreadOwned { owner } => {
            assert_eq!(owner, GPU_MMIO_OWNER, "unknown MMIO owner thread");
            assert_eq!(
                route.device,
                MmioDevice::VirtioGpu,
                "only virtio-gpu has a thread owner for now"
            );
            send_gpu_mmio(devices.gpu_tx, route.offset, size, is_write, value)
        }
    }
}

enum HostInputEvent {
    Key {
        code: u16,
        pressed: bool,
    },
    PointerMove {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    PointerButton {
        button: u16,
        pressed: bool,
        relative: bool,
    },
    Scroll {
        horizontal: bool,
        value: i32,
        relative: bool,
    },
    RelativeMouseMotion {
        dx: i32,
        dy: i32,
    },
}

enum HostDisplayEvent {
    Resize { width: u32, height: u32 },
}

#[derive(Clone, Copy)]
struct VmDevices<'a> {
    uart: &'a Mutex<uart::Uart>,
    blk: &'a Mutex<virtio::MmioTransport<virtio::Blk>>,
    net: &'a Mutex<virtio::MmioTransport<virtio::Net>>,
    gpu_tx: &'a Sender<DeviceThreadRequest>,
    gpu_irq_asserted: &'a AtomicBool,
    keyboard: &'a Mutex<virtio::MmioTransport<virtio::Input>>,
    tablet: &'a Mutex<virtio::MmioTransport<virtio::Input>>,
    mouse: &'a Mutex<virtio::MmioTransport<virtio::Input>>,
    snd: &'a Mutex<virtio::MmioTransport<virtio::Snd>>,
    console: &'a Mutex<virtio::MmioTransport<virtio::Console>>,
}

#[derive(Clone, Copy)]
struct VmIrqs<'a> {
    uart: &'a Mutex<irq::IrqLine>,
    blk: &'a Mutex<irq::IrqLine>,
    net: &'a Mutex<irq::IrqLine>,
    gpu: &'a Mutex<irq::IrqLine>,
    keyboard: &'a Mutex<irq::IrqLine>,
    tablet: &'a Mutex<irq::IrqLine>,
    mouse: &'a Mutex<irq::IrqLine>,
    snd: &'a Mutex<irq::IrqLine>,
    console: &'a Mutex<irq::IrqLine>,
}

fn sync_all_irqs(vm: &VirtualMachineInstance<GicEnabled>, devices: &VmDevices<'_>, irqs: &VmIrqs<'_>) -> Result<()> {
    irqs.uart
        .lock()
        .unwrap()
        .sync(vm, devices.uart.lock().unwrap().is_asserted())?;
    irqs.blk
        .lock()
        .unwrap()
        .sync(vm, devices.blk.lock().unwrap().is_asserted())?;
    irqs.net
        .lock()
        .unwrap()
        .sync(vm, devices.net.lock().unwrap().is_asserted())?;
    irqs.gpu
        .lock()
        .unwrap()
        .sync(vm, devices.gpu_irq_asserted.load(Ordering::SeqCst))?;
    irqs.keyboard
        .lock()
        .unwrap()
        .sync(vm, devices.keyboard.lock().unwrap().is_asserted())?;
    irqs.tablet
        .lock()
        .unwrap()
        .sync(vm, devices.tablet.lock().unwrap().is_asserted())?;
    irqs.mouse
        .lock()
        .unwrap()
        .sync(vm, devices.mouse.lock().unwrap().is_asserted())?;
    irqs.snd
        .lock()
        .unwrap()
        .sync(vm, devices.snd.lock().unwrap().is_asserted())?;
    irqs.console
        .lock()
        .unwrap()
        .sync(vm, devices.console.lock().unwrap().is_asserted())?;
    Ok(())
}

struct HostEventPump<'a> {
    mem: &'a memory::GuestMemory,
    devices: VmDevices<'a>,
    iface: &'a Mutex<net::vmnet::Backend>,
    net_out_rx: Receiver<Vec<u8>>,
    clipboard_in_rx: Receiver<Vec<u8>>,
    input_rx: Receiver<HostInputEvent>,
    display_rx: Receiver<HostDisplayEvent>,
    audio_rx: Receiver<audio::coreaudio::BackendEvent>,
    kicker: kick::Kicker,
    net_buf: Vec<u8>,
}

impl<'a> HostEventPump<'a> {
    fn new(
        mem: &'a memory::GuestMemory,
        devices: VmDevices<'a>,
        iface: &'a Mutex<net::vmnet::Backend>,
        net_out_rx: Receiver<Vec<u8>>,
        clipboard_in_rx: Receiver<Vec<u8>>,
        input_rx: Receiver<HostInputEvent>,
        display_rx: Receiver<HostDisplayEvent>,
        audio_rx: Receiver<audio::coreaudio::BackendEvent>,
        kicker: kick::Kicker,
    ) -> Self {
        let net_buf = vec![0; iface.lock().unwrap().max_packet_size() as usize];

        Self {
            mem,
            devices,
            iface,
            net_out_rx,
            clipboard_in_rx,
            input_rx,
            display_rx,
            audio_rx,
            kicker,
            net_buf,
        }
    }

    fn run(&mut self) {
        loop {
            if self.drain_once() {
                self.kicker.kick();
            }

            std::thread::sleep(std::time::Duration::from_micros(250));
        }
    }

    fn drain_once(&mut self) -> bool {
        let mut touched = false;
        touched |= self.drain_net();
        touched |= self.drain_input();
        touched |= self.drain_display();
        touched |= self.drain_audio();
        touched |= self.drain_clipboard();
        touched
    }

    fn drain_net(&mut self) -> bool {
        let mut touched = false;

        while let Ok(frame) = self.net_out_rx.try_recv() {
            self.iface.lock().unwrap().write(&frame).unwrap();
        }

        loop {
            let n_read = self.iface.lock().unwrap().read(&mut self.net_buf).unwrap();
            if n_read == 0 {
                break;
            }

            touched = true;
            self.devices
                .net
                .lock()
                .unwrap()
                .handle_external_event(&self.net_buf[..n_read], self.mem);
        }

        touched
    }

    fn drain_input(&mut self) -> bool {
        let mut touched = false;
        let mut last_pointer_move: Option<(u32, u32, u32, u32)> = None;

        while let Ok(event) = self.input_rx.try_recv() {
            touched = true;

            use virtio::input::ExternalInput;

            match event {
                HostInputEvent::Key { code, pressed } => {
                    self.devices
                        .keyboard
                        .lock()
                        .unwrap()
                        .handle_external_event(ExternalInput::Key { code, pressed }, self.mem);
                }
                HostInputEvent::PointerMove { x, y, width, height } => {
                    last_pointer_move = Some((x, y, width, height));
                }
                HostInputEvent::PointerButton {
                    button,
                    pressed,
                    relative,
                } => {
                    if relative {
                        self.devices
                            .mouse
                            .lock()
                            .unwrap()
                            .handle_external_event(ExternalInput::PointerButton { button, pressed }, self.mem);
                    } else {
                        if let Some((x, y, width, height)) = last_pointer_move.take() {
                            self.devices
                                .tablet
                                .lock()
                                .unwrap()
                                .handle_external_event(ExternalInput::AbsPosition { x, y, width, height }, self.mem);
                        }

                        self.devices
                            .tablet
                            .lock()
                            .unwrap()
                            .handle_external_event(ExternalInput::PointerButton { button, pressed }, self.mem);
                    }
                }
                HostInputEvent::Scroll {
                    horizontal,
                    value,
                    relative,
                } => {
                    if relative {
                        self.devices
                            .mouse
                            .lock()
                            .unwrap()
                            .handle_external_event(ExternalInput::Scroll { horizontal, value }, self.mem);
                    } else {
                        self.devices
                            .tablet
                            .lock()
                            .unwrap()
                            .handle_external_event(ExternalInput::Scroll { horizontal, value }, self.mem);
                    }
                }
                HostInputEvent::RelativeMouseMotion { dx, dy } => {
                    self.devices
                        .mouse
                        .lock()
                        .unwrap()
                        .handle_external_event(ExternalInput::RelMotion { dx, dy }, self.mem);
                }
            }
        }

        if let Some((x, y, width, height)) = last_pointer_move {
            self.devices.tablet.lock().unwrap().handle_external_event(
                virtio::input::ExternalInput::AbsPosition { x, y, width, height },
                self.mem,
            );
        }

        touched
    }

    fn drain_display(&mut self) -> bool {
        let mut touched = false;

        while let Ok(event) = self.display_rx.try_recv() {
            touched = true;

            match event {
                HostDisplayEvent::Resize { width, height } => {
                    send_gpu_event(
                        self.devices.gpu_tx,
                        virtio::gpu::ExternalEvent::DisplayResized { width, height },
                    );
                }
            }
        }

        touched
    }

    fn drain_audio(&mut self) -> bool {
        let mut touched = false;

        while let Ok(event) = self.audio_rx.try_recv() {
            touched = true;

            match event {
                audio::coreaudio::BackendEvent::PeriodElapsed(seq) => {
                    self.devices
                        .snd
                        .lock()
                        .unwrap()
                        .handle_external_event(virtio::snd::ExternalEvent::PeriodElapsed(seq), self.mem);
                }
            }
        }

        touched
    }

    fn drain_clipboard(&mut self) -> bool {
        let mut touched = false;

        while let Ok(payload) = self.clipboard_in_rx.try_recv() {
            touched = true;

            self.devices
                .console
                .lock()
                .unwrap()
                .handle_external_event(virtio::console::ExternalEvent::HostClipboard(&payload), self.mem);
        }

        touched
    }
}

struct AppState<'a> {
    kicker: kick::Kicker,
    display: &'a Mutex<virtio::gpu::DisplayBuffer>,
    input_tx: Sender<HostInputEvent>,
    display_tx: Sender<HostDisplayEvent>,

    presenter: Option<present::Presenter>,

    surface_w: u32,
    surface_h: u32,

    front: Vec<u32>,
    present_w: usize,
    present_h: usize,
    dirty_rect: Option<(usize, usize, usize, usize)>,
    iosurface_id: Option<u32>,
    last_seq: u64,

    last_mouse_pos: Option<(f64, f64)>,
    mouse_captured: bool,
    grab_mouse_on_click: bool,
    rel_mouse_frac_dx: f64,
    rel_mouse_frac_dy: f64,

    stat_last: std::time::Instant,
    stat_produced: u64,
    stat_presented: u64,
    stat_update_ns: u128,
    stat_loops: u64,
}

impl<'a> AppState<'a> {
    fn kick_vm(&self) {
        self.kicker.kick();
    }

    fn blit(&mut self) -> bool {
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
                );

                self.stat_update_ns += t0.elapsed().as_nanos();
                if presented {
                    self.stat_presented += 1;
                    self.dirty_rect = None;
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
        self.surface_w = width;
        self.surface_h = height;
        self.presenter = Some(present::Presenter::new(window, width, height));

        let _ = self.display_tx.send(HostDisplayEvent::Resize { width, height });
        self.kick_vm();
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

                if let Some(p) = self.presenter.as_mut() {
                    p.resize_surface(width, height);
                }

                let _ = self.display_tx.send(HostDisplayEvent::Resize { width, height });
                self.kick_vm();

                self.blit();
            }

            WindowEvent::RedrawRequested => {
                self.blit();
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
                    if let Some(linux_code) = helpers::winit_to_linux_key(code) {
                        let _ = self.input_tx.send(HostInputEvent::Key {
                            code: linux_code,
                            pressed,
                        });
                        self.kick_vm();
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                if !self.mouse_captured {
                    let PhysicalPosition { x, y } = position;
                    let pos = Some((x, y));
                    if pos != self.last_mouse_pos {
                        self.last_mouse_pos = pos;
                        let _ = self.input_tx.send(HostInputEvent::PointerMove {
                            x: x as u32,
                            y: y as u32,
                            width: self.surface_w,
                            height: self.surface_h,
                        });
                        self.kick_vm();
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
                if state == ElementState::Pressed && self.grab_mouse_on_click {
                    self.set_mouse_capture(true);
                }
                let _ = self.input_tx.send(HostInputEvent::PointerButton {
                    button: btn,
                    pressed: state == ElementState::Pressed,
                    relative: self.mouse_captured,
                });
                self.kick_vm();
            }

            WindowEvent::MouseWheel { delta, .. } => {
                use winit::event::MouseScrollDelta;
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32 / 32.0, p.y as f32 / 32.0),
                };
                if dy != 0.0 {
                    let _ = self.input_tx.send(HostInputEvent::Scroll {
                        horizontal: false,
                        value: dy.round() as i32,
                        relative: self.mouse_captured,
                    });
                }
                if dx != 0.0 {
                    let _ = self.input_tx.send(HostInputEvent::Scroll {
                        horizontal: true,
                        value: dx.round() as i32,
                        relative: self.mouse_captured,
                    });
                }
                self.kick_vm();
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
                let _ = self.input_tx.send(HostInputEvent::RelativeMouseMotion { dx, dy });
                self.kick_vm();
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.poll_display();
        self.blit();

        self.stat_loops += 1;
        self.print_stats();
    }
}

fn build_virglrenderer(
    gpu_tx: Sender<DeviceThreadRequest>,
    kicker: kick::Kicker,
) -> virtio::virgl_ffi::VirglResult<VirglRenderer> {
    let poll_tx = gpu_tx.clone();
    let poll_kicker = kicker.clone();

    let fence_poll_interval = std::env::var("VARMINT_FENCE_POLL_US")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .map(std::time::Duration::from_micros)
        .unwrap_or_else(|| {
            let ms = std::env::var("VARMINT_FENCE_POLL_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|&v| v > 0)
                .unwrap_or(1);
            std::time::Duration::from_millis(ms)
        });

    thread::spawn(move || {
        loop {
            std::thread::sleep(fence_poll_interval);
            if poll_tx
                .send(DeviceThreadRequest::GpuEvent(
                    virtio::gpu::ExternalEvent::PollRendererFences,
                ))
                .is_err()
            {
                break;
            }
            poll_kicker.kick();
        }
    });

    let renderer = VirglRenderer::new(move |fence: VirglFence| {
        let _ = gpu_tx.send(DeviceThreadRequest::GpuEvent(
            virtio::gpu::ExternalEvent::FenceSignaled {
                ctx_id: fence.ctx_id,
                ring_idx: fence.ring_idx.map(|r| r as u8),
                fence_id: fence.fence_id,
            },
        ));
        kicker.kick();
    })?;

    Ok(renderer)
}

fn run_loop(
    vm: &VirtualMachineInstance<GicEnabled>,
    vcpu: &Vcpu,
    vcpu_id: usize,
    mem: &memory::GuestMemory,
    devices: VmDevices<'_>,
    irqs: VmIrqs<'_>,
    net_out_tx: &Sender<Vec<u8>>,
    secondary_boot_txs: &[SyncSender<SecondaryStart>],
    secondary_online: &[AtomicBool],
    clipboard_out_tx: &Sender<Vec<u8>>,
) -> Result<()> {
    loop {
        sync_all_irqs(vm, &devices, &irqs)?;

        vcpu.run()?;
        let exit = vcpu.get_exit_info();

        match exit.reason {
            ExitReason::EXCEPTION => {
                // https://developer.arm.com/documentation/ddi0601/2026-03/AArch64-Registers/ESR-EL2--Exception-Syndrome-Register--EL2-
                let esr_el2_like = exit.exception.syndrome;
                let phys_addr = exit.exception.physical_address;
                let pc = vcpu.get_reg(Reg::PC)?;

                let ec = (esr_el2_like >> 26) & 0b111111;

                match ec {
                    ESR_EC_HVC_AARCH64 => match handle_psci_hvc(vcpu, vcpu_id, secondary_boot_txs, secondary_online)? {
                        PsciAction::Continue => continue,
                        PsciAction::CpuOff => return Ok(()),
                    },

                    0x18 => {
                        if sys_reg::handle_trap(vcpu, esr_el2_like, pc)? {
                            continue;
                        }

                        let (sysreg, rt, is_read) = sys_reg::decode_sysreg(esr_el2_like);
                        panic!(
                            "unhandled sysreg trap: {:?}, rt={}, {}, esr=0x{:x}, pc=0x{:x}",
                            sysreg,
                            rt,
                            if is_read { "read/MRS" } else { "write/MSR" },
                            esr_el2_like,
                            pc
                        );
                    }

                    0x24 | 0x25 => {
                        let is_write = ((esr_el2_like >> 6) & 1) == 1;
                        let rt = (esr_el2_like >> 16) & 0b11111;
                        let size = 1usize << ((esr_el2_like >> 22) & 0b11);

                        let Some(route) = classify(phys_addr) else {
                            panic!(
                                "unhandled data abort trap: ec={}, rt={}, {}, esr=0x{:x}, pc=0x{:x}, addr=0x{:x}",
                                ec,
                                rt,
                                if is_write { "write" } else { "read" },
                                esr_el2_like,
                                pc,
                                phys_addr,
                            );
                        };

                        let value = if is_write { mmio_read_reg(vcpu, rt)? as u64 } else { 0 };
                        if let Some(read_value) = handle_routed_mmio(
                            route,
                            is_write,
                            size,
                            value,
                            mem,
                            &devices,
                            net_out_tx,
                            clipboard_out_tx,
                        ) {
                            mmio_write_reg(vcpu, rt, read_value)?;
                        }

                        vcpu.set_reg(Reg::PC, pc + 4)?;
                    }

                    0x20 | 0x21 => {
                        panic!(
                            "instruction abort: fault=0x{:x}, esr=0x{:x}, pc=0x{:x}",
                            exit.exception.physical_address, esr_el2_like, pc
                        );
                    }

                    _ => {
                        panic!(
                            "unexpected exception ec=0x{:x}, esr=0x{:x}, pc=0x{:x}",
                            ec, esr_el2_like, pc
                        );
                    }
                }
            }
            ExitReason::CANCELED => (),
            _ => eprintln!("unexpected exit reason: {:?}", exit),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn secondary_vcpu_thread(
    vm: &VirtualMachineInstance<GicEnabled>,
    vcpu_id: usize,
    boot_rx: Receiver<SecondaryStart>,
    kicker: kick::Kicker,
    mem: &memory::GuestMemory,
    devices: VmDevices<'_>,
    irqs: VmIrqs<'_>,
    net_out_tx: &Sender<Vec<u8>>,
    secondary_boot_txs: &[SyncSender<SecondaryStart>],
    secondary_online: &[AtomicBool],
    clipboard_out_tx: &Sender<Vec<u8>>,
) -> Result<()> {
    let vcpu = vm.vcpu_create()?;
    vcpu.set_sys_reg(SysReg::ACTLR_EL1, 1 << 1)?; // enable TSO
    vcpu.set_sys_reg(SysReg::MPIDR_EL1, secondary_mpidr(vcpu_id))?;
    kicker.register(vcpu.get_handle());

    while let Ok(start) = boot_rx.recv() {
        vcpu.set_reg(Reg::CPSR, PSTATE_EL1H_DAIF_MASKED)?;
        vcpu.set_reg(Reg::PC, start.entry_point)?;
        vcpu.set_reg(Reg::X0, start.context_id)?;
        vcpu.set_reg(Reg::X1, 0)?;
        vcpu.set_reg(Reg::X2, 0)?;
        vcpu.set_reg(Reg::X3, 0)?;

        run_loop(
            vm,
            &vcpu,
            vcpu_id,
            mem,
            devices,
            irqs,
            net_out_tx,
            secondary_boot_txs,
            secondary_online,
            clipboard_out_tx,
        )?;
    }

    Ok(())
}

fn vmm_thread(
    vm: &VirtualMachineInstance<GicEnabled>,
    kicker_tx: Sender<kick::Kicker>,
    uart: &Mutex<uart::Uart>,
    display: &Mutex<virtio::gpu::DisplayBuffer>,
    input_rx: Receiver<HostInputEvent>,
    display_rx: Receiver<HostDisplayEvent>,
) -> Result<()> {
    let image = read_file("/Users/dvgr/varmint-kernels/debian-4k/vmlinuz-6.12.90+deb13.1-arm64").unwrap();
    let initrd = read_file("/Users/dvgr/varmint-kernels/debian-4k/initrd.img-6.12.90+deb13.1-arm64").unwrap();
    let dtb = read_file("./artifacts/guest.dtb").unwrap();

    let image_header = linux::parse_image_header(&image).unwrap();
    eprintln!("Image header: {:?}", image_header);

    let boot_vcpu = vm.vcpu_create()?;
    boot_vcpu.set_sys_reg(SysReg::ACTLR_EL1, 1 << 1)?; // enable TSO
    boot_vcpu.set_sys_reg(SysReg::MPIDR_EL1, secondary_mpidr(BOOT_VCPU_ID))?;

    let (spi_int_start, _) = GicConfig::get_spi_interrupt_range()?;

    let mut mem = memory::GuestMemory::new(vm.memory_create(RAM_SIZE)?);
    mem.map(RAM_START, MemPerms::RWX)?;
    mem.write(IMAGE_START, &image)?;
    mem.write(INITRD_START, &initrd)?;
    mem.write(DTB_START, &dtb)?;

    boot_vcpu.set_reg(Reg::CPSR, PSTATE_EL1H_DAIF_MASKED)?; // Start in EL1
    boot_vcpu.set_reg(Reg::PC, IMAGE_START)?;
    boot_vcpu.set_reg(Reg::X0, DTB_START)?;
    boot_vcpu.set_reg(Reg::X1, 0)?;
    boot_vcpu.set_reg(Reg::X2, 0)?;
    boot_vcpu.set_reg(Reg::X3, 0)?;

    let uart_irq = Mutex::new(irq::IrqLine::new(spi_int_start + UART_SPI_OFFSET, false));

    let virtio_blk_dev = virtio::Blk::new("dev0.img", 40 * 1024 * 1024 * 1024);
    let virtio_blk = Mutex::new(virtio::MmioTransport::new(virtio_blk_dev));
    let virtio_blk_irq = Mutex::new(irq::IrqLine::new(spi_int_start + VIRTBLK_SPI_OFFSET, false));

    let iface = Mutex::new(net::vmnet::Backend::new().unwrap());
    let virtio_net_dev = virtio::Net::new(iface.lock().unwrap().mac());
    let virtio_net = Mutex::new(virtio::MmioTransport::new(virtio_net_dev));
    let virtio_net_irq = Mutex::new(irq::IrqLine::new(spi_int_start + VIRTNET_SPI_OFFSET, false));

    let virtio_gpu_irq = Mutex::new(irq::IrqLine::new(spi_int_start + VIRTGPU_SPI_OFFSET, false));
    let virtio_gpu_irq_asserted = AtomicBool::new(false);

    let virtio_input_keyboard_dev = virtio::Input::keyboard();
    let virtio_input_keyboard = Mutex::new(virtio::MmioTransport::new(virtio_input_keyboard_dev));
    let virtio_input_keyboard_irq = Mutex::new(irq::IrqLine::new(spi_int_start + VIRTINPUT_KEYBOARD_SPI_OFFSET, false));

    let virtio_input_tablet_dev = virtio::Input::tablet();
    let virtio_input_tablet = Mutex::new(virtio::MmioTransport::new(virtio_input_tablet_dev));
    let virtio_input_tablet_irq = Mutex::new(irq::IrqLine::new(spi_int_start + VIRTINPUT_TABLET_SPI_OFFSET, false));

    let virtio_input_mouse_dev = virtio::Input::mouse();
    let virtio_input_mouse = Mutex::new(virtio::MmioTransport::new(virtio_input_mouse_dev));
    let virtio_input_mouse_irq = Mutex::new(irq::IrqLine::new(spi_int_start + VIRTINPUT_MOUSE_SPI_OFFSET, false));

    let mut secondary_boot_txs = Vec::with_capacity(NUM_VCPUS - 1);
    let mut secondary_boot_rxs = Vec::with_capacity(NUM_VCPUS - 1);
    for _ in FIRST_SECONDARY_VCPU_ID..NUM_VCPUS {
        let (tx, rx) = std::sync::mpsc::sync_channel::<SecondaryStart>(1);
        secondary_boot_txs.push(tx);
        secondary_boot_rxs.push(rx);
    }
    let secondary_online: Vec<AtomicBool> = (FIRST_SECONDARY_VCPU_ID..NUM_VCPUS)
        .map(|_| AtomicBool::new(false))
        .collect();

    let mem_ref = &mem;
    let virtio_gpu_irq_asserted_ref = &virtio_gpu_irq_asserted;

    thread::scope(|s| -> Result<()> {
        let kicker = kick::Kicker::spawn(s, vm, vec![boot_vcpu.get_handle()]);
        kicker_tx.send(kicker.clone()).unwrap();

        let (gpu_tx, gpu_rx) = std::sync::mpsc::channel::<DeviceThreadRequest>();

        let gpu_mem = mem_ref;
        let gpu_display = display;
        let gpu_irq_asserted = virtio_gpu_irq_asserted_ref;
        let gpu_owner_tx = gpu_tx.clone();
        let gpu_kicker = kicker.clone();
        s.spawn(move || {
            gpu_owner_thread(gpu_mem, gpu_display, gpu_rx, gpu_irq_asserted, gpu_owner_tx, gpu_kicker);
        });

        let (_audio_backend, period_sink, audio_rx) = audio::coreaudio::Backend::new(kicker.clone()).unwrap();
        let virtio_snd_dev = virtio::Snd::new(period_sink);
        let virtio_snd = Mutex::new(virtio::MmioTransport::new(virtio_snd_dev));
        let virtio_snd_irq = Mutex::new(irq::IrqLine::new(spi_int_start + VIRTSND_SPI_OFFSET, false));

        let virtio_console_dev = virtio::Console::new();
        let virtio_console = Mutex::new(virtio::MmioTransport::new(virtio_console_dev));
        let virtio_console_irq = Mutex::new(irq::IrqLine::new(spi_int_start + VIRTCONSOLE_SPI_OFFSET, false));

        let (clipboard_in_tx, clipboard_in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (clipboard_out_tx, clipboard_out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (net_out_tx, net_out_rx) = std::sync::mpsc::channel::<Vec<u8>>();

        let clipboard_kicker = kicker.clone();
        s.spawn(move || {
            clipboard::run(clipboard_in_tx, clipboard_out_rx, move || clipboard_kicker.kick());
        });

        let iface_kicker = kicker.clone();
        iface
            .lock()
            .unwrap()
            .set_event_callback(move || iface_kicker.kick())
            .unwrap();

        thread::scope(|ss| -> Result<()> {
            let devices = VmDevices {
                uart,
                blk: &virtio_blk,
                net: &virtio_net,
                gpu_tx: &gpu_tx,
                gpu_irq_asserted: virtio_gpu_irq_asserted_ref,
                keyboard: &virtio_input_keyboard,
                tablet: &virtio_input_tablet,
                mouse: &virtio_input_mouse,
                snd: &virtio_snd,
                console: &virtio_console,
            };
            let irqs = VmIrqs {
                uart: &uart_irq,
                blk: &virtio_blk_irq,
                net: &virtio_net_irq,
                gpu: &virtio_gpu_irq,
                keyboard: &virtio_input_keyboard_irq,
                tablet: &virtio_input_tablet_irq,
                mouse: &virtio_input_mouse_irq,
                snd: &virtio_snd_irq,
                console: &virtio_console_irq,
            };
            let secondary_boot_txs_ref = &secondary_boot_txs;
            let secondary_online_ref = &secondary_online;

            for (secondary_index, boot_rx) in secondary_boot_rxs.into_iter().enumerate() {
                let vcpu_id = FIRST_SECONDARY_VCPU_ID + secondary_index;
                let secondary_kicker = kicker.clone();
                let secondary_net_out_tx = net_out_tx.clone();
                let secondary_clipboard_out_tx = clipboard_out_tx.clone();
                let secondary_devices = devices;
                let secondary_irqs = irqs;

                ss.spawn(move || {
                    secondary_vcpu_thread(
                        vm,
                        vcpu_id,
                        boot_rx,
                        secondary_kicker,
                        mem_ref,
                        secondary_devices,
                        secondary_irqs,
                        &secondary_net_out_tx,
                        secondary_boot_txs_ref,
                        secondary_online_ref,
                        &secondary_clipboard_out_tx,
                    )
                    .unwrap();
                });
            }

            let host_devices = devices;
            let host_kicker = kicker.clone();

            ss.spawn(move || {
                let mut host_events = HostEventPump::new(
                    mem_ref,
                    host_devices,
                    &iface,
                    net_out_rx,
                    clipboard_in_rx,
                    input_rx,
                    display_rx,
                    audio_rx,
                    host_kicker,
                );
                host_events.run();
            });

            run_loop(
                vm,
                &boot_vcpu,
                BOOT_VCPU_ID,
                mem_ref,
                devices,
                irqs,
                &net_out_tx,
                secondary_boot_txs_ref,
                secondary_online_ref,
                &clipboard_out_tx,
            )
        })?;

        Ok(())
    })?;

    Ok(())
}

fn main() -> Result<()> {
    let gicd_size = GicConfig::get_distributor_size()?;
    assert!(GICR_START > GICD_START + gicd_size as u64);

    let mut gic_config = GicConfig::new();
    gic_config.set_distributor_base(GICD_START)?;
    gic_config.set_redistributor_base(GICR_START)?;

    let mut vm_cfg = VirtualMachineConfig::new();
    vm_cfg.set_ipa_granule(IpaGranule::HV_IPA_GRANULE_4KB)?;

    let vm = VirtualMachine::with_gic(vm_cfg, gic_config)?;

    let uart = Mutex::new(uart::Uart::new());
    let display = Mutex::new(virtio::gpu::DisplayBuffer::new());

    let (kicker_tx, kicker_rx) = std::sync::mpsc::channel();
    let (input_tx, input_rx) = std::sync::mpsc::channel();
    let (display_tx, display_rx) = std::sync::mpsc::channel();

    // winit event loop must be created and run on the main thread
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::WaitUntil(
        std::time::Instant::now() + std::time::Duration::from_millis(8),
    ));

    let vm_ref = &vm;
    let uart_ref = &uart;
    let display_ref = &display;

    thread::scope(|s| {
        s.spawn(move || vmm_thread(vm_ref, kicker_tx, uart_ref, display_ref, input_rx, display_rx));

        let kicker = kicker_rx.recv().unwrap();

        let stdin_kicker = kicker.clone();
        s.spawn(move || stdin_thread(stdin_kicker, uart_ref));

        let mut app = AppState {
            kicker,
            display: display_ref,
            input_tx,
            display_tx,
            presenter: None,
            surface_w: 0,
            surface_h: 0,
            front: Vec::new(),
            present_w: 0,
            present_h: 0,
            dirty_rect: None,
            iosurface_id: None,
            last_seq: 0,
            last_mouse_pos: None,
            mouse_captured: false,
            grab_mouse_on_click: std::env::var("VARMINT_GRAB_MOUSE_ON_CLICK")
                .map(|v| v != "0")
                .unwrap_or(false),
            rel_mouse_frac_dx: 0.0,
            rel_mouse_frac_dy: 0.0,
            stat_last: std::time::Instant::now(),
            stat_produced: 0,
            stat_presented: 0,
            stat_update_ns: 0,
            stat_loops: 0,
        };

        event_loop.run_app(&mut app).unwrap();
    });

    Ok(())
}
