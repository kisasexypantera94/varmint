//! The `gamepad` console port: host controllers for the guest's varmint-gamepad service. It
//! creates a uinput gamepad per GameController controller and sends its force feedback back, and
//! a uhid device per raw HID controller (a DualSense), whose reports pass through both ways.
//!
//! Wire format, little-endian: each message is `len: u16, type: u8, payload`, `len` counting
//! `type` and `payload`. Receivers skip unknown types and ignore payload bytes past the fields
//! they know, so fields and messages can be added without a version change.
//!
//!   both ways      HELLO       version: u16, features: u32 (guest → host; bit 0: raw HID)
//!   host → guest   ATTACH      id: u32, name_len: u8, name: [u8; name_len] (UTF-8)
//!                  DETACH      id: u32 (either kind of controller)
//!                  STATE       id: u32, buttons: u32, left_x, left_y, right_x, right_y: i16,
//!                              left_trigger, right_trigger: u16
//!                  HID_ATTACH  id: u32, bus, vendor, product, version: u16, country: u32,
//!                              name_len: u8, name, descriptor_len: u16, descriptor
//!                  HID_INPUT   id: u32, len: u16, report (report ID first)
//!                  HID_REPLY   id: u32, request: u32, errno: u16, len: u16, data
//!   guest → host   RUMBLE      id: u32, strong: u16, weak: u16
//!                  HID_OUTPUT  id: u32, type: u8, len: u16, report
//!                  HID_GET     id: u32, request: u32, type: u8, report_id: u8
//!                  HID_SET     id: u32, request: u32, type: u8, len: u16, report
//!
//! HID report types are uhid's: 0 feature, 1 output, 2 input. HID_GET and HID_SET are answered
//! by one HID_REPLY with the same request (data only for HID_GET; errno 0 on success).
//!
//! STATE buttons are bits by position: 0 south, 1 east, 2 west, 3 north, 4/5 left/right
//! shoulder, 6/7 left/right stick, 8 menu, 9 options, 10 home, 11-14 d-pad up/down/left/right.
//! Sticks are -32767..=32767 with up and right positive; triggers are 0..=65535.
//!
//! The guest opens the port and says HELLO; the host answers HELLO, then ATTACH and STATE for
//! every connected controller. STATE carries the whole state (`gamepad::State`): a newer STATE
//! with the same buttons replaces one the guest has not taken yet, so a guest that falls behind
//! skips stale stick positions but still sees every button press and release. RUMBLE is the
//! guest device's current combined motor level. HID_INPUT is latest-wins as a whole.
//!
//! A guest with raw HID support gets a DualSense as itself (HID_ATTACH) and not also as the
//! GameController controller the host has for it; other guests get the GameController one.

use crate::gamepad::{self, HidIdentity, HidReportType, State, button};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const VERSION: u16 = 1;

/// Larger lengths mean the stream is corrupt. Every message fits a guest receive buffer (a page).
const MAX_MESSAGE_LEN: usize = 4000;

const FEATURE_RAW_HID: u32 = 1 << 0;
const ERRNO_ENODEV: u16 = 19;
/// Beyond this many undelivered messages, keep only each controller's latest state.
const MAX_PENDING: usize = 64;

const WIRE_BUTTONS: [(u32, u32); 15] = [
    (button::SOUTH, 1 << 0),
    (button::EAST, 1 << 1),
    (button::WEST, 1 << 2),
    (button::NORTH, 1 << 3),
    (button::LEFT_SHOULDER, 1 << 4),
    (button::RIGHT_SHOULDER, 1 << 5),
    (button::LEFT_STICK, 1 << 6),
    (button::RIGHT_STICK, 1 << 7),
    (button::MENU, 1 << 8),
    (button::OPTIONS, 1 << 9),
    (button::HOME, 1 << 10),
    (button::DPAD_UP, 1 << 11),
    (button::DPAD_DOWN, 1 << 12),
    (button::DPAD_LEFT, 1 << 13),
    (button::DPAD_RIGHT, 1 << 14),
];

fn wire_buttons(buttons: u32) -> u32 {
    WIRE_BUTTONS
        .iter()
        .filter(|(host, _)| buttons & host != 0)
        .fold(0, |wire, (_, bit)| wire | bit)
}

mod msg {
    pub const HELLO: u8 = 1;
    pub const ATTACH: u8 = 2;
    pub const DETACH: u8 = 3;
    pub const STATE: u8 = 4;
    pub const RUMBLE: u8 = 5;
    pub const HID_ATTACH: u8 = 6;
    pub const HID_INPUT: u8 = 7;
    pub const HID_REPLY: u8 = 8;
    pub const HID_OUTPUT: u8 = 9;
    pub const HID_GET: u8 = 10;
    pub const HID_SET: u8 = 11;
}

fn report_type(wire: u8) -> Option<HidReportType> {
    match wire {
        0 => Some(HidReportType::Feature),
        1 => Some(HidReportType::Output),
        2 => Some(HidReportType::Input),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum GuestMessage {
    Hello {
        version: u16,
        features: u32,
    },
    Rumble {
        id: u32,
        strong: u16,
        weak: u16,
    },
    HidOutput {
        id: u32,
        report_type: HidReportType,
        data: Vec<u8>,
    },
    HidGet {
        id: u32,
        request: u32,
        report_type: HidReportType,
        report_id: u8,
    },
    HidSet {
        id: u32,
        request: u32,
        report_type: HidReportType,
        data: Vec<u8>,
    },
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
struct MalformedStream;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Hello,
    Attach {
        id: u32,
        name: String,
    },
    Detach {
        id: u32,
    },
    State {
        id: u32,
        state: State,
    },
    HidAttach {
        id: u32,
        identity: HidIdentity,
    },
    HidInput {
        id: u32,
        report: Vec<u8>,
    },
    HidReply {
        id: u32,
        request: u32,
        result: Result<Vec<u8>, u16>,
    },
}

impl Pending {
    fn id(&self) -> Option<u32> {
        match self {
            Pending::Hello => None,
            Pending::Attach { id, .. }
            | Pending::Detach { id }
            | Pending::State { id, .. }
            | Pending::HidAttach { id, .. }
            | Pending::HidInput { id, .. }
            | Pending::HidReply { id, .. } => Some(*id),
        }
    }

    /// Whole controller state that a newer message of the same kind replaces.
    fn is_state(&self) -> bool {
        matches!(self, Pending::State { .. } | Pending::HidInput { .. })
    }

    fn encode(&self, out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(&[0, 0]);
        match self {
            Pending::Hello => {
                out.push(msg::HELLO);
                out.extend_from_slice(&VERSION.to_le_bytes());
            }
            Pending::Attach { id, name } => {
                let name = truncate_utf8(name, u8::MAX as usize);
                out.push(msg::ATTACH);
                out.extend_from_slice(&id.to_le_bytes());
                out.push(name.len() as u8);
                out.extend_from_slice(name.as_bytes());
            }
            Pending::Detach { id } => {
                out.push(msg::DETACH);
                out.extend_from_slice(&id.to_le_bytes());
            }
            Pending::State { id, state } => {
                out.push(msg::STATE);
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&wire_buttons(state.buttons).to_le_bytes());
                for axis in [state.left_x, state.left_y, state.right_x, state.right_y] {
                    out.extend_from_slice(&axis.to_le_bytes());
                }
                for trigger in [state.left_trigger, state.right_trigger] {
                    out.extend_from_slice(&trigger.to_le_bytes());
                }
            }
            Pending::HidAttach { id, identity } => {
                let name = truncate_utf8(&identity.name, u8::MAX as usize);
                out.push(msg::HID_ATTACH);
                out.extend_from_slice(&id.to_le_bytes());
                for field in [identity.bus, identity.vendor, identity.product, identity.version] {
                    out.extend_from_slice(&field.to_le_bytes());
                }
                out.extend_from_slice(&identity.country.to_le_bytes());
                out.push(name.len() as u8);
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(&(identity.descriptor.len() as u16).to_le_bytes());
                out.extend_from_slice(&identity.descriptor);
            }
            Pending::HidInput { id, report } => {
                out.push(msg::HID_INPUT);
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&(report.len() as u16).to_le_bytes());
                out.extend_from_slice(report);
            }
            Pending::HidReply { id, request, result } => {
                let (errno, data) = match result {
                    Ok(data) => (0, data.as_slice()),
                    Err(errno) => (*errno, &[][..]),
                };
                out.push(msg::HID_REPLY);
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&request.to_le_bytes());
                out.extend_from_slice(&errno.to_le_bytes());
                out.extend_from_slice(&(data.len() as u16).to_le_bytes());
                out.extend_from_slice(data);
            }
        }
        let len = (out.len() - start - 2) as u16;
        out[start..start + 2].copy_from_slice(&len.to_le_bytes());
    }
}

fn truncate_utf8(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Takes the next complete guest message off `buf`; `Ok(None)` until one is complete.
fn take_guest_message(buf: &mut Vec<u8>) -> Result<Option<GuestMessage>, MalformedStream> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let len = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    if len == 0 || len > MAX_MESSAGE_LEN {
        return Err(MalformedStream);
    }
    if buf.len() < 2 + len {
        return Ok(None);
    }

    let body: Vec<u8> = buf.drain(..2 + len).skip(2).collect();
    let (kind, payload) = (body[0], &body[1..]);
    let u16_at = |at: usize| payload.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |at: usize| {
        payload
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };

    let data_at = |at: usize| {
        let len = u16_at(at)? as usize;
        payload.get(at + 2..at + 2 + len).map(<[u8]>::to_vec)
    };

    let message = match kind {
        msg::HELLO => u16_at(0).map(|version| GuestMessage::Hello {
            version,
            features: u32_at(2).unwrap_or(0),
        }),
        msg::RUMBLE => (|| {
            Some(GuestMessage::Rumble {
                id: u32_at(0)?,
                strong: u16_at(4)?,
                weak: u16_at(6)?,
            })
        })(),
        msg::HID_OUTPUT => (|| {
            Some(GuestMessage::HidOutput {
                id: u32_at(0)?,
                report_type: report_type(*payload.get(4)?)?,
                data: data_at(5)?,
            })
        })(),
        msg::HID_GET => (|| {
            Some(GuestMessage::HidGet {
                id: u32_at(0)?,
                request: u32_at(4)?,
                report_type: report_type(*payload.get(8)?)?,
                report_id: *payload.get(9)?,
            })
        })(),
        msg::HID_SET => (|| {
            Some(GuestMessage::HidSet {
                id: u32_at(0)?,
                request: u32_at(4)?,
                report_type: report_type(*payload.get(8)?)?,
                data: data_at(9)?,
            })
        })(),
        _ => Some(GuestMessage::Unknown),
    };
    // A known message too short for its fields is dropped; the framing is still intact.
    Ok(Some(message.unwrap_or(GuestMessage::Unknown)))
}

/// Host → guest messages not yet delivered, in order.
#[derive(Default)]
struct Outbox {
    pending: VecDeque<Pending>,
}

impl Outbox {
    fn push(&mut self, message: Pending) {
        if message.is_state() {
            let newest_for_id = self
                .pending
                .iter_mut()
                .rev()
                .find(|pending| pending.id() == message.id());
            match (newest_for_id, &message) {
                (Some(Pending::State { state: queued, .. }), Pending::State { state, .. })
                    if queued.buttons == state.buttons =>
                {
                    *queued = *state;
                    return;
                }
                (Some(queued @ Pending::HidInput { .. }), Pending::HidInput { .. }) => {
                    *queued = message;
                    return;
                }
                _ => {}
            }
        }

        self.pending.push_back(message);
        if self.pending.len() > MAX_PENDING {
            self.keep_latest_states();
        }
    }

    /// Drops every state that a later state of the same controller supersedes.
    fn keep_latest_states(&mut self) {
        let mut seen = Vec::new();
        let mut kept = VecDeque::with_capacity(self.pending.len());
        for message in self.pending.drain(..).rev() {
            let id = message.id();
            if message.is_state() {
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
            } else {
                // Other messages end a controller's run of states.
                seen.retain(|seen| *seen != id);
            }
            kept.push_front(message);
        }
        self.pending = kept;
    }

    /// Whole messages, oldest first, that fit in `limit` bytes.
    fn take(&mut self, limit: usize) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(message) = self.pending.front() {
            let end = out.len();
            message.encode(&mut out);
            if out.len() > limit {
                out.truncate(end);
                break;
            }
            self.pending.pop_front();
        }
        out
    }
}

struct Controller {
    name: String,
    state: State,
    /// The host may also pass this controller through as a raw HID device (a DualSense).
    raw_hid: bool,
}

pub struct GamepadPort {
    sink: gamepad::Sink,
    /// Every connected host controller and its latest state, whether or not the guest listens.
    controllers: BTreeMap<u32, Controller>,
    hid_devices: BTreeMap<u32, HidIdentity>,
    /// The ids the guest has been told to attach, of either kind.
    attached: BTreeSet<u32>,
    /// The guest service opened the port and said a compatible HELLO.
    guest_ready: bool,
    guest_raw_hid: bool,
    outbox: Outbox,
    from_guest: Vec<u8>,
}

impl GamepadPort {
    pub fn new(sink: gamepad::Sink) -> GamepadPort {
        GamepadPort {
            sink,
            controllers: BTreeMap::new(),
            hid_devices: BTreeMap::new(),
            attached: BTreeSet::new(),
            guest_ready: false,
            guest_raw_hid: false,
            outbox: Outbox::default(),
            from_guest: Vec::new(),
        }
    }

    /// Whether the guest should have controller `id`. A DualSense is shown through
    /// GameController unless the guest takes raw HID devices and one is actually there, so it
    /// never disappears when the passthrough has nothing for it.
    fn wanted(&self, id: u32) -> bool {
        if !self.guest_ready {
            return false;
        }
        match self.controllers.get(&id) {
            Some(controller) => !(controller.raw_hid && self.guest_raw_hid && !self.hid_devices.is_empty()),
            None => self.guest_raw_hid && self.hid_devices.contains_key(&id),
        }
    }

    /// Detaches and attaches controllers until the guest has exactly the wanted ones.
    fn sync_attached(&mut self) {
        let ids: Vec<u32> = self
            .controllers
            .keys()
            .chain(self.hid_devices.keys())
            .copied()
            .collect();
        let gone: Vec<u32> = self.attached.iter().copied().filter(|&id| !self.wanted(id)).collect();
        for id in gone {
            self.attached.remove(&id);
            // A controller the guest has not been told about yet needs no DETACH.
            let attach_pending = self.outbox.pending.iter().any(|pending| {
                matches!(pending, Pending::Attach { id: queued, .. } | Pending::HidAttach { id: queued, .. } if *queued == id)
            });
            let detach_pending = self
                .outbox
                .pending
                .iter()
                .any(|pending| matches!(pending, Pending::Detach { id: queued } if *queued == id));
            self.outbox.pending.retain(|pending| pending.id() != Some(id));
            if !attach_pending || detach_pending {
                self.outbox.push(Pending::Detach { id });
            }
        }
        for id in ids {
            if self.attached.contains(&id) || !self.wanted(id) {
                continue;
            }
            self.attached.insert(id);
            if let Some(controller) = self.controllers.get(&id) {
                self.outbox.push(Pending::Attach {
                    id,
                    name: controller.name.clone(),
                });
                self.outbox.push(Pending::State {
                    id,
                    state: controller.state,
                });
            } else if let Some(identity) = self.hid_devices.get(&id) {
                self.outbox.push(Pending::HidAttach {
                    id,
                    identity: identity.clone(),
                });
            }
        }
    }

    pub fn on_host_event(&mut self, event: gamepad::Event) {
        match event {
            gamepad::Event::Connected { id, name, raw_hid } => {
                let state = State::default();
                self.controllers.insert(id, Controller { name, state, raw_hid });
                self.sync_attached();
            }
            gamepad::Event::State { id, state } => {
                let Some(controller) = self.controllers.get_mut(&id) else {
                    return;
                };
                controller.state = state;
                if self.attached.contains(&id) {
                    self.outbox.push(Pending::State { id, state });
                }
            }
            gamepad::Event::HidConnected { id, identity } => {
                self.hid_devices.insert(id, identity);
                self.sync_attached();
            }
            gamepad::Event::HidInput { id, report } => {
                if self.attached.contains(&id) {
                    self.outbox.push(Pending::HidInput { id, report });
                }
            }
            gamepad::Event::HidReply { id, request, result } => {
                if self.attached.contains(&id) {
                    self.outbox.push(Pending::HidReply { id, request, result });
                }
            }
            gamepad::Event::Disconnected { id } => {
                self.controllers.remove(&id);
                self.hid_devices.remove(&id);
                self.sync_attached();
            }
        }
    }

    /// The guest opened or closed the port (service start, exit or crash).
    pub fn set_guest_connected(&mut self, connected: bool) {
        if !connected {
            if self.guest_ready {
                eprintln!("gamepad: guest service disconnected");
            }
            self.disconnect();
        }
    }

    pub fn reset(&mut self) {
        self.disconnect();
    }

    fn disconnect(&mut self) {
        self.guest_ready = false;
        self.guest_raw_hid = false;
        self.attached.clear();
        self.outbox.pending.clear();
        self.from_guest.clear();
        self.sink.stop_rumble();
    }

    pub fn receive(&mut self, bytes: &[u8]) {
        self.from_guest.extend_from_slice(bytes);
        loop {
            match take_guest_message(&mut self.from_guest) {
                Ok(Some(message)) => self.handle_guest_message(message),
                Ok(None) => break,
                Err(MalformedStream) => {
                    eprintln!("gamepad: malformed data from the guest service; discarding it");
                    self.from_guest.clear();
                    break;
                }
            }
        }
    }

    fn handle_guest_message(&mut self, message: GuestMessage) {
        match message {
            GuestMessage::Hello { version, features } if version == VERSION => {
                // A new service instance has no devices and plays no rumble yet.
                self.sink.stop_rumble();
                self.guest_ready = true;
                self.guest_raw_hid = features & FEATURE_RAW_HID != 0;
                self.attached.clear();
                self.outbox.pending.clear();
                self.outbox.push(Pending::Hello);
                self.sync_attached();
                eprintln!(
                    "gamepad: guest service ready{}, {} controller(s) shown",
                    if self.guest_raw_hid { " with raw HID" } else { "" },
                    self.attached.len()
                );
            }
            GuestMessage::Hello { version, .. } => {
                eprintln!("gamepad: guest service speaks protocol {version}, host speaks {VERSION}; ignoring it");
                self.guest_ready = false;
                self.attached.clear();
                self.outbox.pending.clear();
            }
            GuestMessage::Rumble { id, strong, weak } => {
                if self.attached.contains(&id) && self.controllers.contains_key(&id) {
                    self.sink.rumble(id, strong, weak);
                }
            }
            GuestMessage::HidOutput { id, data, .. } => {
                if self.attached.contains(&id) && self.hid_devices.contains_key(&id) {
                    self.sink.hid_output(id, data);
                }
            }
            GuestMessage::HidGet {
                id,
                request,
                report_type,
                report_id,
            } => {
                if self.attached.contains(&id) && self.hid_devices.contains_key(&id) {
                    self.sink.hid_get_report(id, request, report_type, report_id);
                } else {
                    self.reply_gone(id, request);
                }
            }
            GuestMessage::HidSet {
                id,
                request,
                report_type,
                data,
            } => {
                if self.attached.contains(&id) && self.hid_devices.contains_key(&id) {
                    self.sink.hid_set_report(id, request, report_type, data);
                } else {
                    self.reply_gone(id, request);
                }
            }
            GuestMessage::Unknown => {}
        }
    }

    /// Answers a request for a device the guest should not have, instead of letting uhid time out.
    fn reply_gone(&mut self, id: u32, request: u32) {
        if self.guest_ready {
            self.outbox.push(Pending::HidReply {
                id,
                request,
                result: Err(ERRNO_ENODEV),
            });
        }
    }

    pub fn has_pending(&self) -> bool {
        !self.outbox.pending.is_empty()
    }

    /// The next bytes for the guest: whole messages that fit in a receive buffer of `limit`.
    pub fn take_for_guest(&mut self, limit: usize) -> Vec<u8> {
        self.outbox.take(limit)
    }
}

#[cfg(test)]
#[path = "gamepad_tests.rs"]
mod tests;
