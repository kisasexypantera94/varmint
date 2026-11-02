//! DualSense controllers as raw HID devices (IOHIDManager), passed through so the guest's own
//! driver (hid-playstation) and Steam drive them: touchpad, motion sensors, lightbar, LEDs,
//! rumble and adaptive triggers all travel in the controller's own reports.
//!
//! Devices are opened without seizing them, so GameController keeps the controller as well (it
//! keeps the PS button from macOS). Manager and input report callbacks run on one serial queue;
//! Get/SetReport block, so they run on a second one and never delay input reports. With a
//! dispatch queue, IOKit activates each device as it matches it and refuses callback
//! registration after activation, so input reports come through the manager's callback.

use super::{Event, Feedback, HidIdentity, HidReportType};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2_core_foundation::{CFArray, CFData, CFDictionary, CFIndex, CFNumber, CFRetained, CFString};
use objc2_io_kit::{IOHIDDevice, IOHIDManager, IOHIDReportType, IOReturn, kIOReturnSuccess};
use std::{
    ffi::c_void,
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

/// Vendor and product IDs hid-playstation binds as a DualSense: DualSense, DualSense Edge.
const SUPPORTED: [(u16, u16); 2] = [(0x054c, 0x0ce6), (0x054c, 0x0df2)];

/// Linux bus types; hid-playstation's report layouts depend on the bus.
const BUS_USB: u16 = 0x03;
const BUS_BLUETOOTH: u16 = 0x05;

/// Leaves room in a guest message for the rest of HID_ATTACH (see virtio::console::gamepad).
const MAX_DESCRIPTOR: usize = 3072;

const ERRNO_EIO: u16 = 5;
const ERRNO_ENODEV: u16 = 19;

pub struct Passthrough {
    requests: Requests,
    manager: CFRetained<IOHIDManager>,
}

/// Report requests for the passed-through devices, from any thread.
#[derive(Clone)]
pub struct Requests {
    queue: DispatchRetained<DispatchQueue>,
    io: DispatchRetained<DispatchQueue>,
    /// Lives as long as the process: the manager's callbacks point at it.
    state: &'static Mutex<State>,
}

struct State {
    emit: Arc<dyn Fn(Event) + Send + Sync>,
    next_id: Arc<AtomicU32>,
    devices: Vec<Device>,
}

// SAFETY: the IOHIDDevices State holds are only used on the passthrough queue.
unsafe impl Send for State {}

struct Device {
    id: u32,
    device: CFRetained<IOHIDDevice>,
    max_feature: usize,
}

struct Handoff(CFRetained<IOHIDDevice>);

// SAFETY: IOHIDDevice report calls may be made from any thread; the I/O queue only makes those.
unsafe impl Send for Handoff {}

impl Passthrough {
    /// None if the HID manager cannot be opened (Input Monitoring denied): DualSenses then reach
    /// the guest through GameController like any other controller.
    pub fn start(emit: Arc<dyn Fn(Event) + Send + Sync>, next_id: Arc<AtomicU32>) -> Option<Passthrough> {
        let queue = DispatchQueue::new("varmint-gamepad-hid", None);
        let io = DispatchQueue::new("varmint-gamepad-hid-io", None);
        let state: &'static Mutex<State> = Box::leak(Box::new(Mutex::new(State {
            emit,
            next_id,
            devices: Vec::new(),
        })));

        let manager = IOHIDManager::new(None, 0);
        let matching: Vec<CFRetained<CFDictionary<CFString, CFNumber>>> = SUPPORTED
            .iter()
            .map(|&(vendor, product)| {
                let (vendor_key, product_key) = (
                    CFString::from_static_str("VendorID"),
                    CFString::from_static_str("ProductID"),
                );
                let (vendor, product) = (CFNumber::new_i32(vendor.into()), CFNumber::new_i32(product.into()));
                CFDictionary::from_slices(&[&*vendor_key, &*product_key], &[&*vendor, &*product])
            })
            .collect();
        let matching = CFArray::from_retained_objects(&matching);
        let context = state as *const Mutex<State> as *mut c_void;
        unsafe {
            manager.set_device_matching_multiple(Some(matching.as_ref()));
            manager.register_device_matching_callback(Some(device_matched), context);
            manager.register_device_removal_callback(Some(device_removed), context);
            manager.register_input_report_callback(Some(input_report), context);
            manager.set_dispatch_queue(&queue);
        }
        let result = manager.open(0);
        if result != kIOReturnSuccess {
            eprintln!(
                "gamepad: no raw HID access ({}); DualSense goes through GameController",
                describe(result)
            );
            return None;
        }
        manager.activate();

        Some(Passthrough {
            requests: Requests { queue, io, state },
            manager,
        })
    }

    pub fn requests(&self) -> Requests {
        self.requests.clone()
    }

    pub fn shutdown(&self) {
        self.manager.cancel();
    }
}

impl Requests {
    pub fn handle(&self, request: Feedback) {
        match request {
            Feedback::HidOutput { id, data } => self.output(id, data),
            Feedback::HidGetReport {
                id,
                request,
                report_type,
                report_id,
            } => self.get_report(id, request, report_type, report_id),
            Feedback::HidSetReport {
                id,
                request,
                report_type,
                data,
            } => self.set_report(id, request, report_type, data),
            Feedback::Rumble { .. } | Feedback::StopAll => {}
        }
    }

    /// An output report (report ID first) for device `id`.
    fn output(&self, id: u32, data: Vec<u8>) {
        self.with_device(id, move |device| {
            if let Some((device, _)) = device {
                if let Err(error) = set_report(&device.0, HidReportType::Output, &data) {
                    eprintln!("gamepad: HID output report: {}", describe(error));
                }
            }
        });
    }

    /// GET_REPORT: replies with `Event::HidReply` (data with the report ID first, or an errno).
    fn get_report(&self, id: u32, request: u32, report_type: HidReportType, report_id: u8) {
        let emit = self.state.lock().unwrap().emit.clone();
        self.with_device(id, move |device| {
            let result = match device {
                Some((device, max_feature)) => get_report(&device.0, report_type, report_id, max_feature),
                None => Err(ERRNO_ENODEV),
            };
            emit(Event::HidReply { id, request, result });
        });
    }

    /// SET_REPORT: replies with an empty `Event::HidReply` or an errno.
    fn set_report(&self, id: u32, request: u32, report_type: HidReportType, data: Vec<u8>) {
        let emit = self.state.lock().unwrap().emit.clone();
        self.with_device(id, move |device| {
            let result = match device {
                Some((device, _)) => set_report(&device.0, report_type, &data)
                    .map(|()| Vec::new())
                    .map_err(|error| {
                        eprintln!("gamepad: HID set report: {}", describe(error));
                        ERRNO_EIO
                    }),
                None => Err(ERRNO_ENODEV),
            };
            emit(Event::HidReply { id, request, result });
        });
    }

    /// Runs blocking report I/O for device `id` on the I/O queue, in request order; `None` if the
    /// device is gone.
    fn with_device(&self, id: u32, work: impl FnOnce(Option<(Handoff, usize)>) + Send + 'static) {
        let (state, io) = (self.state, self.io.clone());
        self.queue.exec_async(move || {
            let device = state
                .lock()
                .unwrap()
                .devices
                .iter()
                .find(|device| device.id == id)
                .map(|device| (Handoff(device.device.clone()), device.max_feature));
            io.exec_async(move || work(device));
        });
    }
}

impl State {
    fn add(&mut self, device: CFRetained<IOHIDDevice>) {
        if self
            .devices
            .iter()
            .any(|d| CFRetained::as_ptr(&d.device) == CFRetained::as_ptr(&device))
        {
            return;
        }
        let Some((identity, max_feature)) = identity(&device) else {
            return;
        };

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        eprintln!(
            "gamepad: {} ({}) passed through as HID controller {id}",
            identity.name,
            if identity.bus == BUS_BLUETOOTH {
                "Bluetooth"
            } else {
                "USB"
            }
        );
        (self.emit)(Event::HidConnected { id, identity });
        self.devices.push(Device {
            id,
            device,
            max_feature,
        });
    }

    fn id_of(&self, device: *mut c_void) -> Option<u32> {
        self.devices
            .iter()
            .find(|d| CFRetained::as_ptr(&d.device).as_ptr().cast::<c_void>() == device)
            .map(|d| d.id)
    }

    fn remove(&mut self, device: NonNull<IOHIDDevice>) {
        let Some(index) = self
            .devices
            .iter()
            .position(|d| CFRetained::as_ptr(&d.device) == device)
        else {
            return;
        };
        let device = self.devices.remove(index);
        eprintln!("gamepad: HID controller {} disconnected", device.id);
        (self.emit)(Event::Disconnected { id: device.id });
    }
}

unsafe extern "C-unwind" fn device_matched(
    context: *mut c_void,
    _: IOReturn,
    _: *mut c_void,
    device: NonNull<IOHIDDevice>,
) {
    let state = unsafe { &*(context as *const Mutex<State>) };
    let device = unsafe { CFRetained::retain(device) };
    state.lock().unwrap().add(device);
}

unsafe extern "C-unwind" fn device_removed(
    context: *mut c_void,
    _: IOReturn,
    _: *mut c_void,
    device: NonNull<IOHIDDevice>,
) {
    let state = unsafe { &*(context as *const Mutex<State>) };
    state.lock().unwrap().remove(device);
}

/// The manager's input report callback; `sender` is the IOHIDDevice.
unsafe extern "C-unwind" fn input_report(
    context: *mut c_void,
    result: IOReturn,
    sender: *mut c_void,
    _: IOHIDReportType,
    _: u32,
    report: NonNull<u8>,
    length: CFIndex,
) {
    if result != kIOReturnSuccess || length <= 0 {
        return;
    }
    let state = unsafe { &*(context as *const Mutex<State>) };
    let state = state.lock().unwrap();
    let Some(id) = state.id_of(sender) else {
        return;
    };
    let report = unsafe { std::slice::from_raw_parts(report.as_ptr(), length as usize) };
    (state.emit)(Event::HidInput {
        id,
        report: report.to_vec(),
    });
}

/// The identity the guest's uhid device gets and the largest feature report; None for a device
/// that cannot be passed through.
fn identity(device: &IOHIDDevice) -> Option<(HidIdentity, usize)> {
    let property = |key: &'static str| device.property(&CFString::from_static_str(key));
    let int = |key| {
        property(key)
            .and_then(|v| v.downcast::<CFNumber>().ok())
            .and_then(|n| n.as_i64())
    };
    let string = |key| {
        property(key)
            .and_then(|v| v.downcast::<CFString>().ok())
            .map(|s| s.to_string())
    };

    let bus = match string("Transport").as_deref() {
        Some("USB") => BUS_USB,
        Some("Bluetooth") => BUS_BLUETOOTH,
        other => {
            eprintln!("gamepad: not passing through a DualSense on transport {other:?}");
            return None;
        }
    };
    let descriptor = property("ReportDescriptor")
        .and_then(|v| v.downcast::<CFData>().ok())
        .map(|d| d.to_vec())
        .filter(|d| !d.is_empty() && d.len() <= MAX_DESCRIPTOR);
    let Some(descriptor) = descriptor else {
        eprintln!("gamepad: not passing through a DualSense without a usable report descriptor");
        return None;
    };

    // The names Linux gives the controller over each bus (hid-playstation does not depend on them).
    let product = string("Product").unwrap_or_else(|| "DualSense Wireless Controller".into());
    let name = match bus {
        BUS_USB => format!(
            "{} {product}",
            string("Manufacturer").unwrap_or_else(|| "Sony Interactive Entertainment".into())
        ),
        _ => product,
    };
    let identity = HidIdentity {
        bus,
        vendor: int("VendorID")? as u16,
        product: int("ProductID")? as u16,
        version: int("VersionNumber").unwrap_or(0) as u16,
        country: int("CountryCode").unwrap_or(0) as u32,
        name,
        descriptor,
    };
    let max_feature = int("MaxFeatureReportSize").unwrap_or(0).max(64) as usize;
    Some((identity, max_feature))
}

fn io_report_type(report_type: HidReportType) -> IOHIDReportType {
    match report_type {
        HidReportType::Feature => IOHIDReportType::Feature,
        HidReportType::Output => IOHIDReportType::Output,
        HidReportType::Input => IOHIDReportType::Input,
    }
}

fn get_report(device: &IOHIDDevice, report_type: HidReportType, report_id: u8, size: usize) -> Result<Vec<u8>, u16> {
    let mut buffer = vec![0u8; size];
    buffer[0] = report_id;
    let mut length = size as CFIndex;
    let result = unsafe {
        device.report(
            io_report_type(report_type),
            report_id.into(),
            NonNull::new(buffer.as_mut_ptr()).unwrap(),
            NonNull::from(&mut length),
        )
    };
    if result != kIOReturnSuccess {
        eprintln!("gamepad: HID get report {report_id:#04x}: {}", describe(result));
        return Err(ERRNO_EIO);
    }
    buffer.truncate(length.clamp(0, size as CFIndex) as usize);
    Ok(buffer)
}

/// `data` starts with the report ID, as Linux passes reports.
fn set_report(device: &IOHIDDevice, report_type: HidReportType, data: &[u8]) -> Result<(), IOReturn> {
    let Some(&report_id) = data.first() else {
        return Ok(());
    };
    let result = unsafe {
        device.set_report(
            io_report_type(report_type),
            report_id.into(),
            NonNull::new(data.as_ptr().cast_mut()).unwrap(),
            data.len() as CFIndex,
        )
    };
    if result == kIOReturnSuccess {
        Ok(())
    } else {
        Err(result)
    }
}

fn describe(result: IOReturn) -> String {
    const NOT_PERMITTED: u32 = 0xe00002e2;
    if result as u32 == NOT_PERMITTED {
        "not permitted: allow Varmint in System Settings > Privacy & Security > Input Monitoring".into()
    } else {
        format!("IOReturn {:#010x}", result as u32)
    }
}
