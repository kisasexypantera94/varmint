use super::Devices;
use crate::{audio, memory::GuestMemory, net, uart, virtio};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::{
    io::Read,
    sync::{Mutex, mpsc::Receiver},
};

pub enum RuntimeInputEvent {
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

pub enum RuntimeEvent {
    NetReady,
    NetTx(Vec<u8>),
    Input(RuntimeInputEvent),
    DisplayResized { width: u32, height: u32 },
    Audio(audio::BackendEvent),
    Clipboard(Vec<u8>),
}

pub struct RuntimeEventPump<'a> {
    mem: &'a GuestMemory,
    devices: &'a Devices,
    iface: net::Backend,
    rx: Receiver<RuntimeEvent>,
    net_buf: Vec<u8>,
}

impl<'a> RuntimeEventPump<'a> {
    pub fn new(mem: &'a GuestMemory, devices: &'a Devices, iface: net::Backend, rx: Receiver<RuntimeEvent>) -> Self {
        let net_buf = vec![0; iface.max_packet_size() as usize];

        Self {
            mem,
            devices,
            iface,
            rx,
            net_buf,
        }
    }

    pub fn run(mut self) {
        while let Ok(event) = self.rx.recv() {
            let mut pointer_move = None;

            self.handle_event(event, &mut pointer_move);

            while let Ok(event) = self.rx.try_recv() {
                self.handle_event(event, &mut pointer_move);
            }

            self.flush_pointer_move(pointer_move);
        }
    }

    fn handle_event(&mut self, event: RuntimeEvent, pointer_move: &mut Option<(u32, u32, u32, u32)>) {
        match event {
            RuntimeEvent::NetReady => self.handle_net_ready(),
            RuntimeEvent::NetTx(frame) => {
                self.iface.write(&frame).unwrap();
            }
            RuntimeEvent::Input(event) => self.handle_input(event, pointer_move),
            RuntimeEvent::DisplayResized { width, height } => {
                self.devices
                    .gpu
                    .send_event(virtio::gpu::ExternalEvent::DisplayResized { width, height });
            }
            RuntimeEvent::Audio(event) => match event {
                audio::BackendEvent::PeriodElapsed(seq) => {
                    self.devices
                        .snd
                        .lock()
                        .unwrap()
                        .handle_external_event(virtio::snd::ExternalEvent::PeriodElapsed(seq), self.mem);
                }
            },
            RuntimeEvent::Clipboard(payload) => {
                self.devices
                    .console
                    .lock()
                    .unwrap()
                    .handle_external_event(virtio::console::ExternalEvent::HostClipboard(&payload), self.mem);
            }
        }
    }

    fn handle_net_ready(&mut self) {
        loop {
            let n_read = self.iface.read(&mut self.net_buf).unwrap();
            if n_read == 0 {
                break;
            }

            self.devices
                .net
                .lock()
                .unwrap()
                .handle_external_event(&self.net_buf[..n_read], self.mem);
        }
    }

    fn handle_input(&mut self, event: RuntimeInputEvent, pointer_move: &mut Option<(u32, u32, u32, u32)>) {
        use virtio::input::ExternalInput;

        let event = match event {
            RuntimeInputEvent::PointerMove { x, y, width, height } => {
                *pointer_move = Some((x, y, width, height));
                return;
            }
            event => event,
        };

        self.flush_pointer_move(pointer_move.take());

        match event {
            RuntimeInputEvent::Key { code, pressed } => {
                self.devices
                    .keyboard
                    .lock()
                    .unwrap()
                    .handle_external_event(ExternalInput::Key { code, pressed }, self.mem);
            }
            RuntimeInputEvent::PointerMove { .. } => unreachable!(),
            RuntimeInputEvent::PointerButton {
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
                    self.devices
                        .tablet
                        .lock()
                        .unwrap()
                        .handle_external_event(ExternalInput::PointerButton { button, pressed }, self.mem);
                }
            }
            RuntimeInputEvent::Scroll {
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
            RuntimeInputEvent::RelativeMouseMotion { dx, dy } => {
                self.devices
                    .mouse
                    .lock()
                    .unwrap()
                    .handle_external_event(ExternalInput::RelMotion { dx, dy }, self.mem);
            }
        }
    }

    fn flush_pointer_move(&mut self, pointer_move: Option<(u32, u32, u32, u32)>) {
        let Some((x, y, width, height)) = pointer_move else {
            return;
        };

        self.devices.tablet.lock().unwrap().handle_external_event(
            virtio::input::ExternalInput::AbsPosition { x, y, width, height },
            self.mem,
        );
    }
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

pub fn run_stdin(uart: &Mutex<uart::Uart>) {
    let _raw = RawModeGuard::new().unwrap();
    let stdin = std::io::stdin();
    let mut buf = [0u8; 1];

    const PREFIX: u8 = 0x1d;
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

                uart.lock().unwrap().enqueue(b);
            }
            Err(e) => {
                eprintln!("stdin read error: {e}");
                break;
            }
        }
    }
}
