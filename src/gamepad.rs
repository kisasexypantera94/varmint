//! Host game controllers (GameController.framework) and their rumble (Core Haptics).
//!
//! GameController input runs on one serial dispatch queue: value-changed handlers run there
//! (`handlerQueue`) and connect/disconnect notifications hop there. Rumble runs on a second one,
//! because Core Haptics makes synchronous XPC calls to gamecontrollerd that must not hold up
//! input. Neither depends on the main thread or the presentation loop.

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::{
    AnyThread, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
};
use objc2_core_haptics::{
    CHHapticDynamicParameter, CHHapticDynamicParameterIDHapticIntensityControl, CHHapticEngine,
    CHHapticEngineStoppedReason, CHHapticEvent, CHHapticEventParameter, CHHapticEventParameterIDHapticIntensity,
    CHHapticEventParameterIDHapticSharpness, CHHapticEventTypeHapticContinuous, CHHapticPattern, CHHapticPatternPlayer,
    CHHapticTimeImmediate,
};
use objc2_foundation::{NSArray, NSError, NSNotification, NSNotificationCenter, NSObjectProtocol};
use objc2_game_controller::{
    GCController, GCControllerDidConnectNotification, GCControllerDidDisconnectNotification, GCControllerElement,
    GCDevice, GCExtendedGamepad, GCHapticDurationInfinite, GCHapticsLocalityDefault, GCHapticsLocalityLeftHandle,
    GCHapticsLocalityRightHandle, GCSystemGestureState,
};
use std::{
    cell::Cell,
    ptr::NonNull,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

mod self_test;

/// Button bits of `State::buttons`, by position (GameController's A is the bottom face button on
/// every controller).
pub mod button {
    pub const SOUTH: u32 = 1 << 0;
    pub const EAST: u32 = 1 << 1;
    pub const WEST: u32 = 1 << 2;
    pub const NORTH: u32 = 1 << 3;
    pub const LEFT_SHOULDER: u32 = 1 << 4;
    pub const RIGHT_SHOULDER: u32 = 1 << 5;
    pub const LEFT_STICK: u32 = 1 << 6;
    pub const RIGHT_STICK: u32 = 1 << 7;
    pub const MENU: u32 = 1 << 8;
    pub const OPTIONS: u32 = 1 << 9;
    pub const HOME: u32 = 1 << 10;
    pub const DPAD_UP: u32 = 1 << 11;
    pub const DPAD_DOWN: u32 = 1 << 12;
    pub const DPAD_LEFT: u32 = 1 << 13;
    pub const DPAD_RIGHT: u32 = 1 << 14;
}

/// A controller's whole input state. Sticks are -32767..=32767 with up and right positive (as
/// GameController reports them); triggers are 0..=65535.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct State {
    pub buttons: u32,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
    pub left_trigger: u16,
    pub right_trigger: u16,
}

pub enum Event {
    /// `id` is never reused within a VMM run.
    Connected {
        id: u32,
        name: String,
    },
    State {
        id: u32,
        state: State,
    },
    Disconnected {
        id: u32,
    },
}

/// Rumble requests for the controllers, from the guest.
pub struct Sink {
    send: Box<dyn Fn(Feedback) + Send>,
}

enum Feedback {
    Rumble { id: u32, strong: u16, weak: u16 },
    StopAll,
}

impl Sink {
    /// Motor levels 0..=65535: strong is the low-frequency motor, weak the high-frequency one.
    pub fn rumble(&self, id: u32, strong: u16, weak: u16) {
        (self.send)(Feedback::Rumble { id, strong, weak });
    }

    pub fn stop_rumble(&self) {
        (self.send)(Feedback::StopAll);
    }

    #[cfg(test)]
    pub fn recording() -> (Sink, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let record = log.clone();
        let sink = Sink {
            send: Box::new(move |feedback| {
                record.lock().unwrap().push(match feedback {
                    Feedback::Rumble { id, strong, weak } => format!("rumble {id} {strong} {weak}"),
                    Feedback::StopAll => "stop".to_string(),
                })
            }),
        };
        (sink, log)
    }
}

/// How long `Gamepads::shutdown` waits for rumble to stop. gamecontrollerd also stops a client's
/// haptics when the process exits, so a wedged haptics queue must not hold up quitting.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

/// The running backend. Its notification observers belong to the thread that called `start`.
pub struct Gamepads {
    input: OnQueue<Bridge>,
    haptics: OnQueue<Haptics>,
    // The queues hold only weak references.
    _bridge: Arc<Mutex<Bridge>>,
    _haptics: Arc<Mutex<Haptics>>,
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    self_test: Option<self_test::SelfTest>,
}

pub fn start(emit: impl Fn(Event) + Send + Sync + 'static) -> Gamepads {
    let emit: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(emit);
    let input_queue = DispatchQueue::new("varmint-gamepad", None);
    let haptics_queue = DispatchQueue::new("varmint-gamepad-haptics", None);

    let haptics_state = Arc::new_cyclic(|this| {
        Mutex::new(Haptics {
            this: OnQueue {
                queue: haptics_queue.clone(),
                state: this.clone(),
            },
            pads: Vec::new(),
        })
    });
    let haptics = OnQueue {
        queue: haptics_queue,
        state: Arc::downgrade(&haptics_state),
    };
    let bridge_state = Arc::new(Mutex::new(Bridge {
        queue: input_queue.clone(),
        emit: emit.clone(),
        haptics: haptics.clone(),
        pads: Vec::new(),
        next_id: 1,
    }));
    let input = OnQueue {
        queue: input_queue,
        state: Arc::downgrade(&bridge_state),
    };

    unsafe { GCController::setShouldMonitorBackgroundEvents(true) };

    let center = NSNotificationCenter::defaultCenter();
    let observers = [unsafe { GCControllerDidConnectNotification }, unsafe {
        GCControllerDidDisconnectNotification
    }]
    .into_iter()
    .map(|name| {
        let input = input.clone();
        let block = RcBlock::new(move |_: NonNull<NSNotification>| input.run(Bridge::reconcile));
        unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) }
    })
    .collect();

    input.run(Bridge::reconcile);

    Gamepads {
        input,
        haptics,
        _bridge: bridge_state,
        _haptics: haptics_state,
        observers,
        self_test: std::env::var_os("VARMINT_GAMEPAD_SELFTEST").map(|_| self_test::SelfTest::start(emit)),
    }
}

impl Gamepads {
    pub fn sink(&self) -> Sink {
        let haptics = self.haptics.clone();
        let self_test = self.self_test.is_some();
        Sink {
            send: Box::new(move |feedback| match feedback {
                Feedback::Rumble { id, strong, weak } if self_test && id == self_test::ID => {
                    eprintln!("gamepad: self-test controller rumble strong={strong} weak={weak}");
                }
                Feedback::Rumble { id, strong, weak } => haptics.run(move |h| h.set(id, Some((strong, weak)))),
                Feedback::StopAll => haptics.run(Haptics::stop_all),
            }),
        }
    }

    /// Releases the controllers and stops their rumble, waiting at most `SHUTDOWN_WAIT`.
    pub fn shutdown(&self) {
        let center = NSNotificationCenter::defaultCenter();
        for observer in &self.observers {
            let observer: &AnyObject = (**observer).as_ref();
            unsafe { center.removeObserver(observer) };
        }
        if let Some(self_test) = &self.self_test {
            self_test.stop();
        }

        let (done_tx, done_rx) = mpsc::channel();
        let haptics = self.haptics.clone();
        self.input.run(move |bridge| {
            bridge.detach_all();
            // Queued after the removals detach_all queued.
            haptics.run(move |haptics| {
                haptics.shut_down();
                let _ = done_tx.send(());
            });
        });
        let _ = done_rx.recv_timeout(SHUTDOWN_WAIT);
    }
}

/// State confined to one serial dispatch queue; work on it can be queued from any thread.
struct OnQueue<T> {
    queue: DispatchRetained<DispatchQueue>,
    state: Weak<Mutex<T>>,
}

impl<T> Clone for OnQueue<T> {
    fn clone(&self) -> Self {
        OnQueue {
            queue: self.queue.clone(),
            state: self.state.clone(),
        }
    }
}

impl<T: Send + 'static> OnQueue<T> {
    fn run(&self, work: impl FnOnce(&mut T) + Send + 'static) {
        let state = self.state.clone();
        self.queue.exec_async(move || {
            if let Some(state) = state.upgrade() {
                work(&mut state.lock().unwrap());
            }
        });
    }
}

/// A GameController object handed from the input queue to the haptics queue.
struct Handoff(Retained<GCController>);

// SAFETY: GameController objects may be messaged from any queue; each queue keeps to its own
// part of the controller (input handlers on one, `haptics` on the other).
unsafe impl Send for Handoff {}

/// Input: the connected controllers. Used only on the input queue.
struct Bridge {
    queue: DispatchRetained<DispatchQueue>,
    emit: Arc<dyn Fn(Event) + Send + Sync>,
    haptics: OnQueue<Haptics>,
    pads: Vec<Pad>,
    next_id: u32,
}

// SAFETY: the GameController objects a Bridge holds are only used on the input queue.
unsafe impl Send for Bridge {}

struct Pad {
    id: u32,
    name: String,
    controller: Retained<GCController>,
}

impl Bridge {
    /// Attaches new extended gamepads and detaches the ones GameController no longer lists.
    fn reconcile(&mut self) {
        let connected: Vec<Retained<GCController>> = unsafe { GCController::controllers() }
            .iter()
            .filter(|controller| unsafe { controller.extendedGamepad() }.is_some())
            .collect();

        let gone: Vec<u32> = self
            .pads
            .iter()
            .filter(|pad| {
                !connected
                    .iter()
                    .any(|c| Retained::as_ptr(c) == Retained::as_ptr(&pad.controller))
            })
            .map(|pad| pad.id)
            .collect();
        for id in gone {
            self.detach(id);
        }

        for controller in connected {
            if !self
                .pads
                .iter()
                .any(|pad| Retained::as_ptr(&pad.controller) == Retained::as_ptr(&controller))
            {
                self.attach(controller);
            }
        }
    }

    fn attach(&mut self, controller: Retained<GCController>) {
        let name = display_name(&controller);
        let Some(gamepad) = (unsafe { controller.extendedGamepad() }) else {
            return;
        };

        let id = self.next_id;
        self.next_id += 1;

        unsafe { controller.setHandlerQueue(&self.queue) };
        // Keep Home, Options and Menu for the guest (Steam button, View, Start) instead of macOS.
        for button in [unsafe { gamepad.buttonHome() }, unsafe { gamepad.buttonOptions() }]
            .into_iter()
            .flatten()
            .chain([unsafe { gamepad.buttonMenu() }])
        {
            unsafe { button.setPreferredSystemGestureState(GCSystemGestureState::Disabled) };
        }

        let initial = read_state(&gamepad);
        let emit = self.emit.clone();
        // The handler also fires for elements without a mapping (a DualSense's touchpad).
        let last = Cell::new(initial);
        let handler = RcBlock::new(
            move |gamepad: NonNull<GCExtendedGamepad>, _: NonNull<GCControllerElement>| {
                let state = read_state(unsafe { gamepad.as_ref() });
                if last.replace(state) != state {
                    emit(Event::State { id, state });
                }
            },
        );
        unsafe { gamepad.setValueChangedHandler(RcBlock::as_ptr(&handler)) };

        let haptics = if unsafe { controller.haptics() }.is_some() {
            ""
        } else {
            ", no haptics: rumble is not played"
        };
        eprintln!("gamepad: {name} connected as controller {id}{haptics}");

        (self.emit)(Event::Connected { id, name: name.clone() });
        (self.emit)(Event::State { id, state: initial });

        let handoff = Handoff(controller.clone());
        let haptics_name = name.clone();
        self.haptics.run(move |haptics| haptics.add(id, haptics_name, handoff));

        self.pads.push(Pad { id, name, controller });
    }

    fn detach(&mut self, id: u32) {
        let Some(index) = self.pads.iter().position(|pad| pad.id == id) else {
            return;
        };
        let pad = self.pads.remove(index);
        if let Some(gamepad) = unsafe { pad.controller.extendedGamepad() } {
            unsafe { gamepad.setValueChangedHandler(std::ptr::null_mut()) };
        }
        self.haptics.run(move |haptics| haptics.remove(id));
        eprintln!("gamepad: {} (controller {id}) disconnected", pad.name);
        (self.emit)(Event::Disconnected { id });
    }

    fn detach_all(&mut self) {
        let ids: Vec<u32> = self.pads.iter().map(|pad| pad.id).collect();
        for id in ids {
            self.detach(id);
        }
    }
}

fn display_name(controller: &GCController) -> String {
    let category = unsafe { controller.productCategory() }.to_string();
    match unsafe { controller.vendorName() } {
        Some(vendor) if vendor.to_string() != category => format!("{vendor} ({category})"),
        _ => category,
    }
}

fn read_state(gamepad: &GCExtendedGamepad) -> State {
    let dpad = unsafe { gamepad.dpad() };
    let left = unsafe { gamepad.leftThumbstick() };
    let right = unsafe { gamepad.rightThumbstick() };
    let (left_trigger, right_trigger) = unsafe { (gamepad.leftTrigger(), gamepad.rightTrigger()) };

    let buttons = [
        (button::SOUTH, Some(unsafe { gamepad.buttonA() })),
        (button::EAST, Some(unsafe { gamepad.buttonB() })),
        (button::WEST, Some(unsafe { gamepad.buttonX() })),
        (button::NORTH, Some(unsafe { gamepad.buttonY() })),
        (button::LEFT_SHOULDER, Some(unsafe { gamepad.leftShoulder() })),
        (button::RIGHT_SHOULDER, Some(unsafe { gamepad.rightShoulder() })),
        (button::LEFT_STICK, unsafe { gamepad.leftThumbstickButton() }),
        (button::RIGHT_STICK, unsafe { gamepad.rightThumbstickButton() }),
        (button::MENU, Some(unsafe { gamepad.buttonMenu() })),
        (button::OPTIONS, unsafe { gamepad.buttonOptions() }),
        (button::HOME, unsafe { gamepad.buttonHome() }),
        (button::DPAD_UP, Some(unsafe { dpad.up() })),
        (button::DPAD_DOWN, Some(unsafe { dpad.down() })),
        (button::DPAD_LEFT, Some(unsafe { dpad.left() })),
        (button::DPAD_RIGHT, Some(unsafe { dpad.right() })),
    ]
    .into_iter()
    .filter(|(_, button)| button.as_ref().is_some_and(|b| unsafe { b.isPressed() }))
    .fold(0, |bits, (bit, _)| bits | bit);

    unsafe {
        State {
            buttons,
            left_x: stick_axis(left.xAxis().value()),
            left_y: stick_axis(left.yAxis().value()),
            right_x: stick_axis(right.xAxis().value()),
            right_y: stick_axis(right.yAxis().value()),
            left_trigger: trigger_axis(left_trigger.value()),
            right_trigger: trigger_axis(right_trigger.value()),
        }
    }
}

fn stick_axis(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

fn trigger_axis(value: f32) -> u16 {
    (value.clamp(0.0, 1.0) * u16::MAX as f32).round() as u16
}

/// Rumble: the controllers' actuators. Used only on the haptics queue.
struct Haptics {
    this: OnQueue<Haptics>,
    pads: Vec<HapticPad>,
}

// SAFETY: the GameController and Core Haptics objects Haptics holds are only used on the haptics
// queue.
unsafe impl Send for Haptics {}

struct HapticPad {
    id: u32,
    name: String,
    controller: Retained<GCController>,
    rumble: Rumble<Motors>,
    announced: bool,
    reported_error: bool,
}

impl Haptics {
    fn add(&mut self, id: u32, name: String, controller: Handoff) {
        if unsafe { controller.0.haptics() }.is_none() {
            return;
        }
        self.pads.push(HapticPad {
            id,
            name,
            controller: controller.0,
            rumble: Rumble::default(),
            announced: false,
            reported_error: false,
        });
    }

    fn remove(&mut self, id: u32) {
        if let Some(index) = self.pads.iter().position(|pad| pad.id == id) {
            self.pads.remove(index).rumble.shut_down();
        }
    }

    /// Plays `level` on controller `id`; `None` plays its current level again.
    fn set(&mut self, id: u32, level: Option<(u16, u16)>) {
        let this = self.this.clone();
        let Some(pad) = self.pads.iter_mut().find(|pad| pad.id == id) else {
            return;
        };
        let level = level.unwrap_or(pad.rumble.level);
        let controller = &pad.controller;
        let mut created = None;
        let result = pad.rumble.play(level, Instant::now(), || {
            let on_engine_stop = move || this.run(move |haptics| haptics.set(id, None));
            let motors = create_motors(controller, on_engine_stop)?;
            created = Some(motors.localities.clone());
            Ok(motors)
        });

        if let Some(localities) = created.filter(|_| !pad.announced) {
            eprintln!("gamepad: rumble on {} ({localities})", pad.name);
            pad.announced = true;
        }
        if let Err(error) = result {
            if !pad.reported_error {
                eprintln!("gamepad: rumble on {}: {error}", pad.name);
                pad.reported_error = true;
            }
        }
    }

    fn stop_all(&mut self) {
        let ids: Vec<u32> = self.pads.iter().map(|pad| pad.id).collect();
        for id in ids {
            self.set(id, Some((0, 0)));
        }
    }

    fn shut_down(&mut self) {
        for mut pad in self.pads.drain(..) {
            pad.rumble.shut_down();
        }
    }
}

/// After actuators fail, how long a controller's rumble requests are dropped before new ones are
/// created: a haptics server that keeps failing is not hammered on every rumble change.
const RETRY_DELAY: Duration = Duration::from_secs(2);

/// What `Rumble` drives: one controller's haptic actuators.
trait Actuators {
    /// Levels 0..=1. An error leaves the actuators unusable.
    fn play(&mut self, strong: f32, weak: f32) -> Result<(), String>;
    /// The system stopped or reset them after they had worked.
    fn stopped(&self) -> bool;
    fn shut_down(&mut self);
}

/// A controller's rumble level and the actuators playing it, created on first use and replaced
/// when they stop or fail.
struct Rumble<A> {
    level: (u16, u16),
    actuators: Option<A>,
    retry_at: Option<Instant>,
}

impl<A> Default for Rumble<A> {
    fn default() -> Self {
        Rumble {
            level: (0, 0),
            actuators: None,
            retry_at: None,
        }
    }
}

impl<A: Actuators> Rumble<A> {
    fn play(
        &mut self,
        level: (u16, u16),
        now: Instant,
        create: impl FnOnce() -> Result<A, String>,
    ) -> Result<(), String> {
        self.level = level;
        if self.actuators.as_ref().is_some_and(A::stopped) {
            self.shut_down();
        }
        if self.actuators.is_none() {
            if level == (0, 0) || self.retry_at.is_some_and(|at| now < at) {
                return Ok(());
            }
            match create() {
                Ok(actuators) => self.actuators = Some(actuators),
                Err(error) => {
                    self.retry_at = Some(now + RETRY_DELAY);
                    return Err(error);
                }
            }
        }

        let strong = level.0 as f32 / u16::MAX as f32;
        let weak = level.1 as f32 / u16::MAX as f32;
        let result = self.actuators.as_mut().map_or(Ok(()), |a| a.play(strong, weak));
        if result.is_err() {
            self.shut_down();
            self.retry_at = Some(now + RETRY_DELAY);
        }
        result
    }

    fn shut_down(&mut self) {
        if let Some(mut actuators) = self.actuators.take() {
            actuators.shut_down();
        }
    }
}

/// The strong (low-frequency) motor on the left handle and the weak one on the right, where an
/// Xbox pad has them; a single actuator plays the stronger of the two.
struct Motors {
    motors: Vec<Motor>,
    localities: String,
}

impl Actuators for Motors {
    fn play(&mut self, strong: f32, weak: f32) -> Result<(), String> {
        let intensities = match self.motors.len() {
            2 => vec![strong, weak],
            _ => vec![strong.max(weak)],
        };
        for (motor, intensity) in self.motors.iter_mut().zip(intensities) {
            motor.play(intensity).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn stopped(&self) -> bool {
        self.motors.iter().any(|motor| motor.stopped.load(Ordering::Acquire))
    }

    fn shut_down(&mut self) {
        for motor in &mut self.motors {
            motor.shut_down();
        }
    }
}

fn create_motors(controller: &GCController, on_engine_stop: impl Fn() + Clone + 'static) -> Result<Motors, String> {
    let haptics = unsafe { controller.haptics() }.ok_or("no haptics")?;
    let localities = unsafe { haptics.supportedLocalities() };
    let (left, right) = unsafe { (GCHapticsLocalityLeftHandle, GCHapticsLocalityRightHandle) };
    // Low sharpness for the strong motor, high for the weak one.
    let plan = if localities.containsObject(left) && localities.containsObject(right) {
        vec![(left, 0.2), (right, 0.8)]
    } else {
        vec![(unsafe { GCHapticsLocalityDefault }, 0.5)]
    };

    let mut motors = Vec::with_capacity(plan.len());
    for (locality, sharpness) in &plan {
        // objc2-game-controller only binds this for iOS-family targets; macOS has it since 11.
        let engine: Option<Retained<CHHapticEngine>> =
            unsafe { msg_send![&*haptics, createEngineWithLocality: *locality] };
        let engine = engine.ok_or_else(|| format!("no haptic engine for {locality}"))?;
        motors.push(Motor::new(engine, *sharpness, on_engine_stop.clone()));
    }
    let localities: Vec<String> = plan.iter().map(|(locality, _)| locality.to_string()).collect();
    Ok(Motors {
        motors,
        localities: localities.join(", "),
    })
}

/// One actuator group: an endless continuous haptic event whose intensity follows the guest.
///
/// It uses a plain pattern player: on macOS 27, gamecontrollerd refuses advanced players (the
/// request fails with NSCocoaErrorDomain 4097 and leaves the engine unusable).
struct Motor {
    engine: Retained<CHHapticEngine>,
    sharpness: f32,
    player: Option<Retained<ProtocolObject<dyn CHHapticPatternPlayer>>>,
    playing: bool,
    /// Set by the engine's stopped and reset handlers once the engine has played.
    stopped: Arc<AtomicBool>,
    worked: Arc<AtomicBool>,
}

impl Motor {
    fn new(engine: Retained<CHHapticEngine>, sharpness: f32, on_engine_stop: impl Fn() + Clone + 'static) -> Motor {
        let stopped = Arc::new(AtomicBool::new(false));
        let worked = Arc::new(AtomicBool::new(false));

        let handler = {
            let (stopped, worked) = (stopped.clone(), worked.clone());
            move || {
                // An engine that never played failed; `Rumble` replaces it after its retry delay.
                if worked.load(Ordering::Acquire) {
                    stopped.store(true, Ordering::Release);
                    on_engine_stop();
                }
            }
        };
        let on_stopped = handler.clone();
        let stopped_handler = RcBlock::new(move |_: CHHapticEngineStoppedReason| on_stopped());
        let reset_handler = RcBlock::new(handler);
        unsafe {
            engine.setStoppedHandler(RcBlock::as_ptr(&stopped_handler));
            engine.setResetHandler(RcBlock::as_ptr(&reset_handler));
        }

        Motor {
            engine,
            sharpness,
            player: None,
            playing: false,
            stopped,
            worked,
        }
    }

    /// 0..=1; 0 stops.
    fn play(&mut self, intensity: f32) -> Result<(), Retained<NSError>> {
        if intensity <= 0.0 {
            if self.playing {
                self.playing = false;
                if let Some(player) = &self.player {
                    unsafe { player.stopAtTime_error(CHHapticTimeImmediate) }?;
                }
            }
            return Ok(());
        }

        let player = match &self.player {
            Some(player) => player.clone(),
            None => {
                unsafe { self.engine.startAndReturnError() }?;
                let player = self.make_player()?;
                self.player = Some(player.clone());
                player
            }
        };

        let control = unsafe {
            CHHapticDynamicParameter::initWithParameterID_value_relativeTime(
                CHHapticDynamicParameter::alloc(),
                CHHapticDynamicParameterIDHapticIntensityControl,
                intensity,
                0.0,
            )
        };
        let controls = NSArray::from_retained_slice(&[control]);
        if !self.playing {
            unsafe { player.startAtTime_error(CHHapticTimeImmediate) }?;
            self.playing = true;
        }
        unsafe { player.sendParameters_atTime_error(&controls, CHHapticTimeImmediate) }?;
        self.worked.store(true, Ordering::Release);
        Ok(())
    }

    fn make_player(&self) -> Result<Retained<ProtocolObject<dyn CHHapticPatternPlayer>>, Retained<NSError>> {
        unsafe {
            let parameters = NSArray::from_retained_slice(&[
                CHHapticEventParameter::initWithParameterID_value(
                    CHHapticEventParameter::alloc(),
                    CHHapticEventParameterIDHapticIntensity,
                    1.0,
                ),
                CHHapticEventParameter::initWithParameterID_value(
                    CHHapticEventParameter::alloc(),
                    CHHapticEventParameterIDHapticSharpness,
                    self.sharpness,
                ),
            ]);
            let event = CHHapticEvent::initWithEventType_parameters_relativeTime_duration(
                CHHapticEvent::alloc(),
                CHHapticEventTypeHapticContinuous,
                &parameters,
                0.0,
                GCHapticDurationInfinite as f64,
            );
            let pattern = CHHapticPattern::initWithEvents_parameters_error(
                CHHapticPattern::alloc(),
                &NSArray::from_retained_slice(&[event]),
                &NSArray::new(),
            )?;
            self.engine.createPlayerWithPattern_error(&pattern)
        }
    }

    fn shut_down(&mut self) {
        unsafe {
            if self.playing {
                if let Some(player) = &self.player {
                    let _ = player.stopAtTime_error(CHHapticTimeImmediate);
                }
            }
            self.engine.setStoppedHandler(std::ptr::null_mut());
            self.engine.setResetHandler(std::ptr::null_mut());
            self.engine.stopWithCompletionHandler(std::ptr::null_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_axis_reaches_both_extrema_and_centers() {
        assert_eq!(stick_axis(-1.0), -32767);
        assert_eq!(stick_axis(0.0), 0);
        assert_eq!(stick_axis(1.0), 32767);
        assert_eq!(stick_axis(1.5), 32767, "clamped");
        assert_eq!(stick_axis(-1.5), -32767, "clamped");
    }

    #[test]
    fn trigger_axis_is_analog_over_the_full_range() {
        assert_eq!(trigger_axis(0.0), 0);
        assert_eq!(trigger_axis(0.5), 32768);
        assert_eq!(trigger_axis(1.0), 65535);
        assert_eq!(trigger_axis(-0.1), 0, "clamped");
    }

    /// Records what `Rumble` asks of its actuators.
    struct FakeActuators {
        log: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
        fail: bool,
        stopped: std::rc::Rc<Cell<bool>>,
    }

    impl Actuators for FakeActuators {
        fn play(&mut self, strong: f32, weak: f32) -> Result<(), String> {
            self.log.borrow_mut().push(format!("play {strong:.1} {weak:.1}"));
            if self.fail { Err("refused".into()) } else { Ok(()) }
        }

        fn stopped(&self) -> bool {
            self.stopped.get()
        }

        fn shut_down(&mut self) {
            self.log.borrow_mut().push("shut down".into());
        }
    }

    struct Harness {
        rumble: Rumble<FakeActuators>,
        log: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
        stopped: std::rc::Rc<Cell<bool>>,
        fail: bool,
        start: Instant,
    }

    impl Harness {
        fn new() -> Harness {
            Harness {
                rumble: Rumble::default(),
                log: Default::default(),
                stopped: Default::default(),
                fail: false,
                start: Instant::now(),
            }
        }

        fn play(&mut self, level: (u16, u16), at_ms: u64) -> Result<(), String> {
            let (log, stopped, fail) = (self.log.clone(), self.stopped.clone(), self.fail);
            self.rumble.play(level, self.start + Duration::from_millis(at_ms), || {
                log.borrow_mut().push("create".into());
                stopped.set(false);
                Ok(FakeActuators { log, fail, stopped })
            })
        }

        fn take_log(&self) -> Vec<String> {
            std::mem::take(&mut self.log.borrow_mut())
        }
    }

    #[test]
    fn rumble_creates_actuators_only_for_a_nonzero_level() {
        let mut h = Harness::new();
        h.play((0, 0), 0).unwrap();
        assert!(h.take_log().is_empty());
        h.play((u16::MAX, 0), 1).unwrap();
        h.play((0, u16::MAX), 2).unwrap();
        h.play((0, 0), 3).unwrap();
        assert_eq!(h.take_log(), ["create", "play 1.0 0.0", "play 0.0 1.0", "play 0.0 0.0"]);
    }

    #[test]
    fn failed_actuators_are_replaced_only_after_the_retry_delay() {
        let mut h = Harness::new();
        h.fail = true;
        assert!(h.play((u16::MAX, 0), 0).is_err());
        assert_eq!(h.take_log(), ["create", "play 1.0 0.0", "shut down"]);

        h.fail = false;
        let retry = RETRY_DELAY.as_millis() as u64;
        for at in [1, 100, retry - 1] {
            h.play((u16::MAX, u16::MAX), at).unwrap();
        }
        assert!(h.take_log().is_empty(), "no new engines while backing off");

        h.play((u16::MAX, u16::MAX), retry).unwrap();
        assert_eq!(h.take_log(), ["create", "play 1.0 1.0"]);
    }

    #[test]
    fn stopped_actuators_are_replaced_at_once_with_the_current_level() {
        let mut h = Harness::new();
        h.play((u16::MAX, 0), 0).unwrap();
        h.take_log();
        h.stopped.set(true);
        // The stopped handler replays the current level.
        let level = h.rumble.level;
        h.play(level, 1).unwrap();
        assert_eq!(h.take_log(), ["shut down", "create", "play 1.0 0.0"]);
    }

    #[test]
    fn stopped_actuators_at_rest_are_dropped_without_new_ones() {
        let mut h = Harness::new();
        h.play((u16::MAX, 0), 0).unwrap();
        h.play((0, 0), 1).unwrap();
        h.take_log();
        h.stopped.set(true);
        h.play((0, 0), 2).unwrap();
        assert_eq!(h.take_log(), ["shut down"]);
    }
}
