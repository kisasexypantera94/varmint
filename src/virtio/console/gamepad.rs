//! The `gamepad` console port: host controllers for the guest's varmint-gamepad service, which
//! creates a uinput gamepad per controller and sends its force feedback back.
//!
//! Wire format, little-endian: each message is `len: u16, type: u8, payload`, `len` counting
//! `type` and `payload`. Receivers skip unknown types and ignore payload bytes past the fields
//! they know, so fields and messages can be added without a version change.
//!
//!   both ways      HELLO   version: u16
//!   host → guest   ATTACH  id: u32, name_len: u8, name: [u8; name_len] (UTF-8)
//!                  DETACH  id: u32
//!                  STATE   id: u32, buttons: u32, left_x, left_y, right_x, right_y: i16,
//!                          left_trigger, right_trigger: u16
//!   guest → host   RUMBLE  id: u32, strong: u16, weak: u16
//!
//! STATE buttons are bits by position: 0 south, 1 east, 2 west, 3 north, 4/5 left/right
//! shoulder, 6/7 left/right stick, 8 menu, 9 options, 10 home, 11-14 d-pad up/down/left/right.
//! Sticks are -32767..=32767 with up and right positive; triggers are 0..=65535.
//!
//! The guest opens the port and says HELLO; the host answers HELLO, then ATTACH and STATE for
//! every connected controller. STATE carries the whole state (`gamepad::State`): a newer STATE
//! with the same buttons replaces one the guest has not taken yet, so a guest that falls behind
//! skips stale stick positions but still sees every button press and release. RUMBLE is the
//! guest device's current combined motor level.

use crate::gamepad::{self, State, button};
use std::collections::{BTreeMap, VecDeque};

pub const VERSION: u16 = 1;

/// Larger lengths mean the stream is corrupt.
const MAX_MESSAGE_LEN: usize = 512;
/// Beyond this many undelivered messages, keep only each controller's latest state.
const MAX_PENDING: usize = 64;

/// `gamepad::button` bits and their STATE bits.
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
}

#[derive(Debug, PartialEq, Eq)]
enum GuestMessage {
    Hello { version: u16 },
    Rumble { id: u32, strong: u16, weak: u16 },
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
struct MalformedStream;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Hello,
    Attach { id: u32, name: String },
    Detach { id: u32 },
    State { id: u32, state: State },
}

impl Pending {
    fn id(&self) -> Option<u32> {
        match self {
            Pending::Hello => None,
            Pending::Attach { id, .. } | Pending::Detach { id } | Pending::State { id, .. } => Some(*id),
        }
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

    let message = match kind {
        msg::HELLO => u16_at(0).map(|version| GuestMessage::Hello { version }),
        msg::RUMBLE => (|| {
            Some(GuestMessage::Rumble {
                id: u32_at(0)?,
                strong: u16_at(4)?,
                weak: u16_at(6)?,
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
        if let Pending::State { id, state } = &message {
            let newest_for_id = self.pending.iter_mut().rev().find(|pending| pending.id() == Some(*id));
            if let Some(Pending::State { state: queued, .. }) = newest_for_id {
                if queued.buttons == state.buttons {
                    *queued = *state;
                    return;
                }
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
            match &message {
                Pending::State { id, .. } if seen.contains(id) => continue,
                Pending::State { id, .. } => seen.push(*id),
                // Lifecycle messages end a controller's run of states.
                _ => seen.retain(|id| Some(*id) != message.id()),
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

pub struct GamepadPort {
    sink: gamepad::Sink,
    /// Every connected host controller and its latest state, whether or not the guest listens.
    controllers: BTreeMap<u32, (String, State)>,
    /// The guest service opened the port and said a compatible HELLO.
    guest_ready: bool,
    outbox: Outbox,
    from_guest: Vec<u8>,
}

impl GamepadPort {
    pub fn new(sink: gamepad::Sink) -> GamepadPort {
        GamepadPort {
            sink,
            controllers: BTreeMap::new(),
            guest_ready: false,
            outbox: Outbox::default(),
            from_guest: Vec::new(),
        }
    }

    pub fn on_host_event(&mut self, event: gamepad::Event) {
        match event {
            gamepad::Event::Connected { id, name } => {
                if self.guest_ready {
                    self.outbox.push(Pending::Attach { id, name: name.clone() });
                }
                self.controllers.insert(id, (name, State::default()));
            }
            gamepad::Event::State { id, state } => {
                let Some((_, current)) = self.controllers.get_mut(&id) else {
                    return;
                };
                *current = state;
                if self.guest_ready {
                    self.outbox.push(Pending::State { id, state });
                }
            }
            gamepad::Event::Disconnected { id } => {
                if self.controllers.remove(&id).is_some() && self.guest_ready {
                    // A controller the guest has not been told about yet needs no DETACH.
                    let attach_pending = self
                        .outbox
                        .pending
                        .iter()
                        .any(|pending| matches!(pending, Pending::Attach { id: queued, .. } if *queued == id));
                    self.outbox.pending.retain(|pending| pending.id() != Some(id));
                    if !attach_pending {
                        self.outbox.push(Pending::Detach { id });
                    }
                }
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
            GuestMessage::Hello { version } if version == VERSION => {
                eprintln!(
                    "gamepad: guest service ready, {} controller(s) connected",
                    self.controllers.len()
                );
                // A new service instance has no devices and plays no rumble yet.
                self.sink.stop_rumble();
                self.guest_ready = true;
                self.outbox.pending.clear();
                self.outbox.push(Pending::Hello);
                for (&id, (name, state)) in &self.controllers {
                    self.outbox.push(Pending::Attach { id, name: name.clone() });
                    self.outbox.push(Pending::State { id, state: *state });
                }
            }
            GuestMessage::Hello { version } => {
                eprintln!("gamepad: guest service speaks protocol {version}, host speaks {VERSION}; ignoring it");
                self.guest_ready = false;
                self.outbox.pending.clear();
            }
            GuestMessage::Rumble { id, strong, weak } => {
                if self.guest_ready && self.controllers.contains_key(&id) {
                    self.sink.rumble(id, strong, weak);
                }
            }
            GuestMessage::Unknown => {}
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
