use crate::virtio::{
    chain::ChainData,
    common,
    device::{ChainAction, ChainToken, Device, Effect, ExternalEventHandler},
    input::keys::*,
};
use applevisor::memory::Memory;
use num_enum::TryFromPrimitive;
use zerocopy::{FromBytes, Immutable, IntoBytes};

pub mod keys;

const DEVICE_ID: u32 = 18;

const TABLET_ABS_MAX_X: u32 = 32767;
const TABLET_ABS_MAX_Y: u32 = 32767;

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(usize)]
enum QueueType {
    Event = 0,
    Status = 1,
}

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-2450006
#[derive(IntoBytes, FromBytes, Immutable)]
#[repr(C, packed)]
struct Event {
    r#type: u16,
    code: u16,
    value: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C)]
struct InputConfig {
    select: u8,
    subsel: u8,
    size: u8,
    reserved: [u8; 5],
    u: [u8; 128],
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct InputDevids {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct AbsInfo {
    min: u32,
    max: u32,
    fuzz: u32,
    flat: u32,
    res: u32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
enum InputConfigSelect {
    IdName = 0x01,
    IdSerial = 0x02,
    IdDevids = 0x03,
    PropBits = 0x10,
    EvBits = 0x11,
    AbsInfo = 0x12,
}

enum InputKind {
    Keyboard,
    Tablet,
}

pub struct Input {
    kind: InputKind,
    select: u8,
    subsel: u8,
}

fn set_bit(bitmap: &mut [u8], bit: usize) {
    bitmap[bit / 8] |= 1 << (bit % 8);
}

impl Input {
    pub fn keyboard() -> Input {
        Input {
            kind: InputKind::Keyboard,
            select: 0,
            subsel: 0,
        }
    }

    pub fn tablet() -> Input {
        Input {
            kind: InputKind::Tablet,
            select: 0,
            subsel: 0,
        }
    }

    fn config(&self) -> InputConfig {
        let mut cfg = InputConfig {
            select: self.select,
            subsel: self.subsel,
            size: 0,
            reserved: [0; 5],
            u: [0; 128],
        };

        match InputConfigSelect::try_from(self.select) {
            Ok(InputConfigSelect::IdName) => {
                let name = match self.kind {
                    InputKind::Keyboard => "varmint keyboard",
                    InputKind::Tablet => "varmint tablet",
                };
                cfg.size = name.len() as u8;
                cfg.u[..name.len()].copy_from_slice(name.as_bytes());
            }

            Ok(InputConfigSelect::IdDevids) => {
                let devids = InputDevids {
                    bustype: BUS_VIRTUAL,
                    vendor: 0x1234,
                    product: 1,
                    version: 1,
                };

                let bytes = devids.as_bytes();
                cfg.size = bytes.len() as u8;
                cfg.u[..bytes.len()].copy_from_slice(bytes);
            }

            Ok(InputConfigSelect::AbsInfo) => {
                if let InputKind::Tablet = self.kind {
                    let abs = match self.subsel as u16 {
                        ABS_X => Some(AbsInfo {
                            min: 0,
                            max: TABLET_ABS_MAX_X,
                            fuzz: 0,
                            flat: 0,
                            res: 0,
                        }),
                        ABS_Y => Some(AbsInfo {
                            min: 0,
                            max: TABLET_ABS_MAX_Y,
                            fuzz: 0,
                            flat: 0,
                            res: 0,
                        }),
                        _ => None,
                    };

                    if let Some(abs) = abs {
                        let bytes = abs.as_bytes();
                        cfg.size = bytes.len() as u8;
                        cfg.u[..bytes.len()].copy_from_slice(bytes);
                    }
                }
            }

            Ok(InputConfigSelect::EvBits) => match self.kind {
                InputKind::Keyboard => match self.subsel as u16 {
                    EV_SYN => {
                        set_bit(&mut cfg.u, SYN_REPORT as usize);
                        cfg.size = 1;
                    }
                    EV_KEY => {
                        cfg.size = fill_key_bits(&mut cfg.u);
                    }
                    _ => {}
                },

                InputKind::Tablet { .. } => match self.subsel as u16 {
                    EV_SYN => {
                        set_bit(&mut cfg.u, SYN_REPORT as usize);
                        cfg.size = 1;
                    }
                    EV_KEY => {
                        set_bit(&mut cfg.u, BTN_LEFT as usize);
                        set_bit(&mut cfg.u, BTN_RIGHT as usize);
                        set_bit(&mut cfg.u, BTN_MIDDLE as usize);

                        cfg.size = (BTN_MIDDLE / 8 + 1) as u8;
                    }
                    EV_ABS => {
                        set_bit(&mut cfg.u, ABS_X as usize);
                        set_bit(&mut cfg.u, ABS_Y as usize);

                        cfg.size = 1;
                    }
                    EV_REL => {
                        set_bit(&mut cfg.u, REL_WHEEL as usize);
                        set_bit(&mut cfg.u, REL_HWHEEL as usize);

                        cfg.size = 2;
                    }
                    _ => {}
                },
            },

            Ok(InputConfigSelect::PropBits) => {
                if matches!(self.kind, InputKind::Tablet { .. }) {
                    set_bit(&mut cfg.u, INPUT_PROP_POINTER as usize);
                    cfg.size = 1;
                }
            }

            Ok(InputConfigSelect::IdSerial) => {
                cfg.size = 0;
            }

            Err(e) => {
                eprintln!("unexpected select: {}", e);
                cfg.size = 0;
            }
        }

        cfg
    }
}

impl Device for Input {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        let offset = offset as usize;
        data.copy_from_slice(&self.config().as_bytes()[offset..offset + data.len()]);
    }

    fn write_config(&mut self, offset: u64, data: &[u8]) {
        for (i, &byte) in data.iter().enumerate() {
            match offset + i as u64 {
                0 => self.select = byte,
                1 => self.subsel = byte,
                _ => {}
            }
        }
    }

    fn num_queues(&self) -> u16 {
        2
    }

    fn delivery_queues(&self) -> &[u16] {
        &[QueueType::Event as u16]
    }

    fn process_chain(
        &mut self,
        queue_idx: usize,
        _chain: &ChainData,
        _token: ChainToken,
        _mem: &mut Memory,
    ) -> ChainAction {
        match QueueType::try_from(queue_idx).unwrap() {
            QueueType::Event => ChainAction::Complete(0),
            QueueType::Status => ChainAction::Complete(0),
        }
    }

    fn reset(&mut self) {
        self.select = 0;
        self.subsel = 0;
    }
}

pub enum ExternalInput {
    Key {
        code: u16,
        pressed: bool,
    },
    PointerButton {
        button: u16,
        pressed: bool,
    },
    AbsPosition {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Scroll {
        horizontal: bool,
        value: i32,
    },
}

impl ExternalEventHandler for Input {
    type Event<'a> = ExternalInput;

    fn on_event(&mut self, input: ExternalInput, mut emit: impl FnMut(Effect)) {
        let q = QueueType::Event as usize;
        let mut ev = |t: u16, c: u16, v: u32| {
            emit(Effect::Deliver {
                queue_idx: q,
                parts: &[Event {
                    r#type: t,
                    code: c,
                    value: v,
                }
                .as_bytes()],
            });
        };

        match input {
            ExternalInput::Key { code, pressed } => {
                ev(EV_KEY, code, pressed as u32);
                ev(EV_SYN, SYN_REPORT, 0);
            }
            ExternalInput::PointerButton { button, pressed } => {
                ev(EV_KEY, button, pressed as u32);
                ev(EV_SYN, SYN_REPORT, 0);
            }
            ExternalInput::AbsPosition {
                x,
                y,
                width,
                height,
            } => {
                let x = (x as u64 * TABLET_ABS_MAX_X as u64) / (width as u64 - 1);
                let y = (y as u64 * TABLET_ABS_MAX_Y as u64) / (height as u64 - 1);
                ev(EV_ABS, ABS_X, x as u32);
                ev(EV_ABS, ABS_Y, y as u32);
                ev(EV_SYN, SYN_REPORT, 0);
            }
            ExternalInput::Scroll { horizontal, value } => {
                let code = if horizontal { REL_HWHEEL } else { REL_WHEEL };
                ev(EV_REL, code, value as u32);
                ev(EV_SYN, SYN_REPORT, 0);
            }
        }
    }
}
