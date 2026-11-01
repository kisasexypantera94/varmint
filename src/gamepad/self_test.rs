//! VARMINT_GAMEPAD_SELFTEST: a synthetic controller for checking the guest side without a real
//! one. It connects, presses South and pushes the left stick and right trigger ten times,
//! disconnects, and repeats until `Gamepads::shutdown`.

use super::{Event, State, button};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

/// Never handed out to real controllers, whose ids start at 1.
pub const ID: u32 = 0;

const STEP: Duration = Duration::from_millis(500);

pub struct SelfTest {
    stop: Arc<AtomicBool>,
}

impl SelfTest {
    pub fn start(emit: Arc<dyn Fn(Event) + Send + Sync>) -> SelfTest {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        thread::Builder::new()
            .name("varmint-gamepad-selftest".into())
            .spawn(move || run(&*emit, &stopped))
            .unwrap_or_else(|error| panic!("failed to spawn gamepad self-test thread: {error}"));
        SelfTest { stop }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn run(emit: &(dyn Fn(Event) + Send + Sync), stop: &AtomicBool) {
    let pressed = State {
        buttons: button::SOUTH,
        left_x: i16::MAX,
        right_trigger: u16::MAX,
        ..State::default()
    };
    let step = || {
        thread::sleep(STEP);
        !stop.load(Ordering::Acquire)
    };

    loop {
        eprintln!("gamepad: self-test controller connected");
        emit(Event::Connected {
            id: ID,
            name: "Varmint self-test".into(),
        });
        for state in [pressed, State::default()].into_iter().cycle().take(20) {
            emit(Event::State { id: ID, state });
            if !step() {
                break;
            }
        }
        eprintln!("gamepad: self-test controller disconnected");
        emit(Event::Disconnected { id: ID });
        if !(0..6).all(|_| step()) {
            return;
        }
    }
}
