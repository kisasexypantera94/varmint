use super::*;
use crate::gamepad::{Event, Sink};
use std::sync::{Arc, Mutex};

fn port() -> (GamepadPort, Arc<Mutex<Vec<String>>>) {
    let (sink, feedback) = Sink::recording();
    (GamepadPort::new(sink), feedback)
}

fn guest_hello(version: u16) -> Vec<u8> {
    let mut bytes = vec![3, 0, msg::HELLO];
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes
}

fn guest_rumble(id: u32, strong: u16, weak: u16) -> Vec<u8> {
    let mut bytes = vec![9, 0, msg::RUMBLE];
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(&strong.to_le_bytes());
    bytes.extend_from_slice(&weak.to_le_bytes());
    bytes
}

/// Decodes host → guest bytes into readable lines.
fn decode(mut bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        let (kind, p) = (bytes[2], &bytes[3..2 + len]);
        let u32_at = |at: usize| u32::from_le_bytes(p[at..at + 4].try_into().unwrap());
        let i16_at = |at: usize| i16::from_le_bytes(p[at..at + 2].try_into().unwrap());
        let u16_at = |at: usize| u16::from_le_bytes(p[at..at + 2].try_into().unwrap());
        out.push(match kind {
            msg::HELLO => format!("hello {}", u16_at(0)),
            msg::ATTACH => format!(
                "attach {} {}",
                u32_at(0),
                String::from_utf8_lossy(&p[5..5 + p[4] as usize])
            ),
            msg::DETACH => format!("detach {}", u32_at(0)),
            msg::STATE => format!(
                "state {} {:#x} {} {} {} {} {} {}",
                u32_at(0),
                u32_at(4),
                i16_at(8),
                i16_at(10),
                i16_at(12),
                i16_at(14),
                u16_at(16),
                u16_at(18)
            ),
            other => format!("type {other}"),
        });
        bytes = &bytes[2 + len..];
    }
    out
}

fn drain(port: &mut GamepadPort) -> Vec<String> {
    decode(&port.take_for_guest(4096))
}

fn pressed(buttons: u32) -> State {
    State {
        buttons,
        ..State::default()
    }
}

fn stick(x: i16) -> State {
    State {
        left_x: x,
        ..State::default()
    }
}

fn connect(port: &mut GamepadPort, id: u32, name: &str) {
    port.on_host_event(Event::Connected { id, name: name.into() });
}

fn ready(port: &mut GamepadPort) {
    port.receive(&guest_hello(VERSION));
}

/// Shared with the guest's tests (tests/guest/test_varmint_gamepad.py).
const VECTORS: &str = include_str!("../../../tests/guest/gamepad-protocol-vectors.json");

fn vectors(direction: &str) -> Vec<serde_json::Value> {
    let all: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
    all[direction].as_array().unwrap().clone()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn host_button(name: &str) -> u32 {
    match name {
        "south" => button::SOUTH,
        "east" => button::EAST,
        "west" => button::WEST,
        "north" => button::NORTH,
        "left_shoulder" => button::LEFT_SHOULDER,
        "right_shoulder" => button::RIGHT_SHOULDER,
        "left_stick" => button::LEFT_STICK,
        "right_stick" => button::RIGHT_STICK,
        "menu" => button::MENU,
        "options" => button::OPTIONS,
        "home" => button::HOME,
        "dpad_up" => button::DPAD_UP,
        "dpad_down" => button::DPAD_DOWN,
        "dpad_left" => button::DPAD_LEFT,
        "dpad_right" => button::DPAD_RIGHT,
        other => panic!("unknown button {other}"),
    }
}

#[test]
fn host_messages_encode_as_the_shared_vectors() {
    let field = |v: &serde_json::Value, name: &str| v[name].as_i64().unwrap();
    for v in vectors("host_to_guest") {
        let id = || field(&v, "id") as u32;
        let message = match v["message"].as_str().unwrap() {
            "hello" => Pending::Hello,
            "attach" => Pending::Attach {
                id: id(),
                name: v["name"].as_str().unwrap().into(),
            },
            "detach" => Pending::Detach { id: id() },
            "state" => Pending::State {
                id: id(),
                state: State {
                    buttons: v["buttons"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .fold(0, |bits, name| bits | host_button(name.as_str().unwrap())),
                    left_x: field(&v, "left_x") as i16,
                    left_y: field(&v, "left_y") as i16,
                    right_x: field(&v, "right_x") as i16,
                    right_y: field(&v, "right_y") as i16,
                    left_trigger: field(&v, "left_trigger") as u16,
                    right_trigger: field(&v, "right_trigger") as u16,
                },
            },
            other => panic!("unknown message {other}"),
        };
        let mut bytes = Vec::new();
        message.encode(&mut bytes);
        assert_eq!(hex(&bytes), v["bytes"].as_str().unwrap(), "{v}");
    }
}

#[test]
fn guest_messages_decode_from_the_shared_vectors() {
    for v in vectors("guest_to_host") {
        let mut bytes = unhex(v["bytes"].as_str().unwrap());
        let message = take_guest_message(&mut bytes).unwrap().unwrap();
        let expected = match v["message"].as_str().unwrap() {
            "hello" => GuestMessage::Hello {
                version: v["version"].as_u64().unwrap() as u16,
            },
            "rumble" => GuestMessage::Rumble {
                id: v["id"].as_u64().unwrap() as u32,
                strong: v["strong"].as_u64().unwrap() as u16,
                weak: v["weak"].as_u64().unwrap() as u16,
            },
            other => panic!("unknown message {other}"),
        };
        assert_eq!(message, expected, "{v}");
        assert!(bytes.is_empty());
    }
}

#[test]
fn attach_name_is_length_prefixed_and_truncated_on_a_char_boundary() {
    let mut bytes = Vec::new();
    Pending::Attach {
        id: 1,
        name: "é".repeat(200),
    }
    .encode(&mut bytes);
    let name_len = bytes[7] as usize;
    assert_eq!(name_len, 254);
    assert!(std::str::from_utf8(&bytes[8..8 + name_len]).is_ok());
    assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]) as usize, bytes.len() - 2);
}

#[test]
fn nothing_is_sent_before_the_guest_says_hello() {
    let (mut port, _) = port();
    connect(&mut port, 1, "Pad");
    port.on_host_event(Event::State {
        id: 1,
        state: pressed(button::SOUTH),
    });
    assert!(!port.has_pending());
}

#[test]
fn hello_replays_connected_controllers_with_their_latest_state() {
    let (mut port, feedback) = port();
    connect(&mut port, 1, "DualSense");
    port.on_host_event(Event::State {
        id: 1,
        state: pressed(button::NORTH),
    });
    connect(&mut port, 2, "Xbox");
    ready(&mut port);
    assert_eq!(
        drain(&mut port),
        [
            "hello 1",
            "attach 1 DualSense",
            "state 1 0x8 0 0 0 0 0 0",
            "attach 2 Xbox",
            "state 2 0x0 0 0 0 0 0 0",
        ]
    );
    assert_eq!(
        *feedback.lock().unwrap(),
        ["stop"],
        "a new guest service starts without rumble"
    );
}

#[test]
fn hotplug_after_hello_is_forwarded_in_order() {
    let (mut port, _) = port();
    ready(&mut port);
    drain(&mut port);
    connect(&mut port, 3, "Pad");
    port.on_host_event(Event::State {
        id: 3,
        state: pressed(button::EAST),
    });
    assert_eq!(drain(&mut port), ["attach 3 Pad", "state 3 0x2 0 0 0 0 0 0"]);
    port.on_host_event(Event::Disconnected { id: 3 });
    connect(&mut port, 4, "Pad");
    assert_eq!(drain(&mut port), ["detach 3", "attach 4 Pad"]);
}

#[test]
fn analog_changes_coalesce_but_button_transitions_survive() {
    let (mut port, _) = port();
    connect(&mut port, 1, "Pad");
    ready(&mut port);
    drain(&mut port);

    for x in [100, 200, 300] {
        port.on_host_event(Event::State { id: 1, state: stick(x) });
    }
    port.on_host_event(Event::State {
        id: 1,
        state: pressed(button::SOUTH),
    });
    port.on_host_event(Event::State {
        id: 1,
        state: stick(400),
    });
    port.on_host_event(Event::State {
        id: 1,
        state: stick(500),
    });

    assert_eq!(
        drain(&mut port),
        [
            "state 1 0x0 300 0 0 0 0 0",
            "state 1 0x1 0 0 0 0 0 0",
            "state 1 0x0 500 0 0 0 0 0",
        ],
        "a tap between stick moves is kept; intermediate stick positions are not"
    );
}

#[test]
fn coalescing_is_per_controller() {
    let (mut port, _) = port();
    connect(&mut port, 1, "A");
    connect(&mut port, 2, "B");
    ready(&mut port);
    drain(&mut port);

    port.on_host_event(Event::State { id: 1, state: stick(1) });
    port.on_host_event(Event::State { id: 2, state: stick(2) });
    port.on_host_event(Event::State { id: 1, state: stick(3) });
    assert_eq!(drain(&mut port), ["state 1 0x0 3 0 0 0 0 0", "state 2 0x0 2 0 0 0 0 0"]);
}

#[test]
fn a_stalled_guest_queue_stays_bounded() {
    let (mut port, _) = port();
    connect(&mut port, 1, "Pad");
    ready(&mut port);
    drain(&mut port);

    for i in 0..10_000 {
        let buttons = if i % 2 == 0 { button::SOUTH } else { 0 };
        port.on_host_event(Event::State {
            id: 1,
            state: State {
                buttons,
                left_x: i as i16,
                ..State::default()
            },
        });
    }
    assert!(port.outbox.pending.len() <= MAX_PENDING);
    let sent = drain(&mut port);
    assert_eq!(
        sent.last().unwrap(),
        "state 1 0x0 9999 0 0 0 0 0",
        "the latest state is never dropped"
    );
}

#[test]
fn a_controller_gone_before_its_attach_was_delivered_is_never_announced() {
    let (mut port, _) = port();
    ready(&mut port);
    drain(&mut port);
    connect(&mut port, 5, "Pad");
    port.on_host_event(Event::State {
        id: 5,
        state: pressed(button::SOUTH),
    });
    port.on_host_event(Event::Disconnected { id: 5 });
    assert!(!port.has_pending());
}

#[test]
fn disconnect_drops_undelivered_states_and_states_after_it_are_ignored() {
    let (mut port, _) = port();
    connect(&mut port, 1, "Pad");
    ready(&mut port);
    drain(&mut port);

    port.on_host_event(Event::State {
        id: 1,
        state: pressed(button::SOUTH),
    });
    port.on_host_event(Event::Disconnected { id: 1 });
    port.on_host_event(Event::State {
        id: 1,
        state: pressed(button::EAST),
    });
    assert_eq!(drain(&mut port), ["detach 1"]);
}

#[test]
fn messages_are_never_split_across_receive_buffers() {
    let (mut port, _) = port();
    for id in 1..=3 {
        connect(&mut port, id, "Pad");
    }
    ready(&mut port);

    let hello_and_attach = 5 + (2 + 1 + 4 + 1 + 3);
    let first = port.take_for_guest(hello_and_attach + 10);
    assert_eq!(decode(&first), ["hello 1", "attach 1 Pad"]);
    let rest = decode(&port.take_for_guest(4096));
    assert_eq!(rest.len(), 5);
    assert!(!port.has_pending());
}

#[test]
fn guest_rumble_reaches_connected_controllers_only() {
    let (mut port, feedback) = port();
    connect(&mut port, 1, "Pad");
    port.receive(&guest_rumble(1, 100, 200));
    ready(&mut port);
    port.receive(&guest_rumble(1, 300, 400));
    port.receive(&guest_rumble(9, 1, 1));
    assert_eq!(*feedback.lock().unwrap(), ["stop", "rumble 1 300 400"]);
}

#[test]
fn guest_messages_may_arrive_in_pieces_and_batches() {
    let (mut port, feedback) = port();
    connect(&mut port, 1, "Pad");
    let mut bytes = guest_hello(VERSION);
    bytes.extend(guest_rumble(1, 1, 2));
    bytes.extend(guest_rumble(1, 3, 4));
    for byte in &bytes[..7] {
        port.receive(std::slice::from_ref(byte));
    }
    port.receive(&bytes[7..]);
    assert_eq!(*feedback.lock().unwrap(), ["stop", "rumble 1 1 2", "rumble 1 3 4"]);
}

#[test]
fn malformed_guest_data_is_discarded() {
    let (mut port, feedback) = port();
    connect(&mut port, 1, "Pad");
    ready(&mut port);

    port.receive(&[0, 0, 0xff]);
    port.receive(&guest_rumble(1, 5, 5));
    port.receive(&[0xff, 0xff, msg::RUMBLE]);
    port.receive(&guest_rumble(1, 6, 6));
    // Truncated RUMBLE and unknown types are skipped without losing the framing.
    port.receive(&[3, 0, msg::RUMBLE, 1, 0]);
    port.receive(&[2, 0, 0x7f, 0]);
    port.receive(&guest_rumble(1, 7, 7));
    // Extra trailing fields of a known message are ignored.
    let mut longer = guest_rumble(1, 8, 8);
    longer[0] += 2;
    longer.extend([0xaa, 0xbb]);
    port.receive(&longer);

    assert_eq!(
        *feedback.lock().unwrap(),
        ["stop", "rumble 1 5 5", "rumble 1 6 6", "rumble 1 7 7", "rumble 1 8 8"]
    );
}

#[test]
fn incompatible_guest_version_gets_no_controllers() {
    let (mut port, _) = port();
    connect(&mut port, 1, "Pad");
    port.receive(&guest_hello(VERSION + 1));
    assert!(!port.has_pending());
}

#[test]
fn guest_service_restart_stops_rumble_and_resynchronizes() {
    let (mut port, feedback) = port();
    connect(&mut port, 1, "Pad");
    ready(&mut port);
    drain(&mut port);
    port.on_host_event(Event::State {
        id: 1,
        state: pressed(button::HOME),
    });

    port.set_guest_connected(false);
    assert!(!port.has_pending(), "nothing is queued for a closed port");
    port.on_host_event(Event::State {
        id: 1,
        state: pressed(button::MENU),
    });
    assert!(!port.has_pending());

    port.set_guest_connected(true);
    ready(&mut port);
    assert_eq!(
        drain(&mut port),
        ["hello 1", "attach 1 Pad", "state 1 0x100 0 0 0 0 0 0"]
    );
    assert_eq!(*feedback.lock().unwrap(), ["stop", "stop", "stop"]);
}

#[test]
fn device_reset_stops_rumble() {
    let (mut port, feedback) = port();
    connect(&mut port, 1, "Pad");
    ready(&mut port);
    port.reset();
    assert!(!port.has_pending());
    assert_eq!(feedback.lock().unwrap().last().unwrap(), "stop");
}
