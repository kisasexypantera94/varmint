use super::Devices;
use crate::{audio, memory::GuestMemory, virtio};
use std::sync::mpsc::Receiver;

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
    NetRx(Vec<u8>),
    UartRx(u8),
    Input(RuntimeInputEvent),
    DisplayResized { width: u32, height: u32 },
    Clipboard(Vec<u8>),
}

pub struct RuntimeEventPump<'a> {
    mem: &'a GuestMemory,
    devices: &'a Devices,
    rx: Receiver<RuntimeEvent>,
}

impl<'a> RuntimeEventPump<'a> {
    pub fn new(mem: &'a GuestMemory, devices: &'a Devices, rx: Receiver<RuntimeEvent>) -> Self {
        Self { mem, devices, rx }
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
            RuntimeEvent::NetRx(frame) => {
                self.devices.net.lock().unwrap().handle_external_event(&frame, self.mem);
            }
            RuntimeEvent::UartRx(byte) => self.devices.uart.lock().unwrap().enqueue(byte),
            RuntimeEvent::Input(event) => self.handle_input(event, pointer_move),
            RuntimeEvent::DisplayResized { width, height } => {
                self.devices
                    .gpu
                    .send_event(virtio::gpu::ExternalEvent::DisplayResized { width, height });
            }
            RuntimeEvent::Clipboard(payload) => {
                self.devices
                    .console
                    .lock()
                    .unwrap()
                    .handle_external_event(virtio::console::ExternalEvent::HostClipboard(&payload), self.mem);
            }
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

pub struct AudioEventPump<'a> {
    mem: &'a GuestMemory,
    devices: &'a Devices,
    rx: Receiver<audio::BackendEvent>,
}

impl<'a> AudioEventPump<'a> {
    pub fn new(mem: &'a GuestMemory, devices: &'a Devices, rx: Receiver<audio::BackendEvent>) -> Self {
        Self { mem, devices, rx }
    }

    pub fn run(self) {
        while let Ok(event) = self.rx.recv() {
            match event {
                audio::BackendEvent::PeriodElapsed(seq) => {
                    self.devices
                        .snd
                        .lock()
                        .unwrap()
                        .handle_external_event(virtio::snd::ExternalEvent::PeriodElapsed(seq), self.mem);
                }
            }
        }
    }
}
