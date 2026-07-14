use crate::{
    audio,
    devices::{VmDevices, send_gpu_event},
    memory::GuestMemory,
    net, virtio,
};
use std::sync::{Mutex, mpsc::Receiver};

pub(crate) enum HostInputEvent {
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

pub(crate) enum HostEvent {
    NetReady,
    NetTx(Vec<u8>),
    Input(HostInputEvent),
    DisplayResized { width: u32, height: u32 },
    Audio(audio::coreaudio::BackendEvent),
    Clipboard(Vec<u8>),
}

pub(crate) struct HostEventPump<'a> {
    mem: &'a GuestMemory,
    devices: VmDevices<'a>,
    iface: &'a Mutex<net::vmnet::Backend>,
    rx: Receiver<HostEvent>,
    net_buf: Vec<u8>,
}

impl<'a> HostEventPump<'a> {
    pub(crate) fn new(
        mem: &'a GuestMemory,
        devices: VmDevices<'a>,
        iface: &'a Mutex<net::vmnet::Backend>,
        rx: Receiver<HostEvent>,
    ) -> Self {
        let net_buf = vec![0; iface.lock().unwrap().max_packet_size() as usize];

        Self {
            mem,
            devices,
            iface,
            rx,
            net_buf,
        }
    }

    pub(crate) fn run(&mut self) {
        while let Ok(event) = self.rx.recv() {
            let mut pointer_move = None;

            self.handle_event(event, &mut pointer_move);

            while let Ok(event) = self.rx.try_recv() {
                self.handle_event(event, &mut pointer_move);
            }

            self.flush_pointer_move(pointer_move);
        }
    }

    fn handle_event(&mut self, event: HostEvent, pointer_move: &mut Option<(u32, u32, u32, u32)>) {
        match event {
            HostEvent::NetReady => self.handle_net_ready(),
            HostEvent::NetTx(frame) => {
                self.iface.lock().unwrap().write(&frame).unwrap();
            }
            HostEvent::Input(event) => self.handle_input(event, pointer_move),
            HostEvent::DisplayResized { width, height } => {
                send_gpu_event(
                    self.devices.gpu_tx,
                    virtio::gpu::ExternalEvent::DisplayResized { width, height },
                );
            }
            HostEvent::Audio(event) => match event {
                audio::coreaudio::BackendEvent::PeriodElapsed(seq) => {
                    self.devices
                        .snd
                        .lock()
                        .unwrap()
                        .handle_external_event(virtio::snd::ExternalEvent::PeriodElapsed(seq), self.mem);
                }
            },
            HostEvent::Clipboard(payload) => {
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
            let n_read = self.iface.lock().unwrap().read(&mut self.net_buf).unwrap();
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

    fn handle_input(&mut self, event: HostInputEvent, pointer_move: &mut Option<(u32, u32, u32, u32)>) {
        use virtio::input::ExternalInput;

        let event = match event {
            HostInputEvent::PointerMove { x, y, width, height } => {
                *pointer_move = Some((x, y, width, height));
                return;
            }
            event => event,
        };

        self.flush_pointer_move(pointer_move.take());

        match event {
            HostInputEvent::Key { code, pressed } => {
                self.devices
                    .keyboard
                    .lock()
                    .unwrap()
                    .handle_external_event(ExternalInput::Key { code, pressed }, self.mem);
            }
            HostInputEvent::PointerMove { .. } => unreachable!(),
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
