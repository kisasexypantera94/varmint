mod vm_log;
mod vms;

use crate::config::{NeptuneBackend, VmConfigDefaults, VmConfigDocument};
use block2::RcBlock;
use objc2::{
    ClassType, DefinedClass, MainThreadOnly, Message, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAppearance, NSAppearanceNameDarkAqua, NSApplication,
    NSApplicationActivationOptions, NSApplicationActivationPolicy, NSApplicationDelegate, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSBox, NSBoxType, NSButton, NSColor, NSControl, NSControlSize, NSControlStateValueOn,
    NSControlTextEditingDelegate, NSEventModifierFlags, NSEventType, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSGridCell, NSGridCellPlacement, NSGridView, NSImage, NSLayoutAttribute,
    NSLineBreakMode, NSMenu, NSMenuItem, NSMenuItemValidation, NSModalResponse, NSModalResponseOK,
    NSMutableParagraphStyle, NSOpenPanel, NSParagraphStyleAttributeName, NSPopUpButton, NSRunningApplication,
    NSScrollView, NSSlider, NSStackView, NSText, NSTextAlignment, NSTextField, NSTextFieldDelegate, NSTextView,
    NSTitlebarAccessoryViewController, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSBundle, NSDictionary, NSNotification, NSObject, NSObjectProtocol,
    NSPoint, NSRange, NSRect, NSSize, NSString, NSTimer, NSURL, ns_string,
};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    process::Child,
};
use vm_log::VmLog;
use vms::{
    create_vm_config, default_vms_directory, duplicate_vm_files, first_known_vm, forget_vm, is_managed_vm, known_vms,
    lock_storage, remember_vm, rename_vm_files, replace_known_vm, sidebar_vms, storage_dir, trash_managed_vm,
    unused_vm_name, vm_name,
};

const GIB: u64 = 1024 * 1024 * 1024;
const D3DMETAL_PATH_VARIABLE: &str = "D3DMETAL_FRAMEWORK_PATH";
const D3DMETAL_DOWNLOAD_URL: &str =
    "https://developer.apple.com/download/all/?q=evaluation%20environment%20for%20windows%20games";
const DISK_PRESETS_GIB: &[u64] = &[64, 128, 256, 512, 1024];
const FORM_WIDTH: f64 = 640.0;
const FORM_HEIGHT: f64 = 380.0;
const SIDEBAR_WIDTH: f64 = 190.0;
const SIDEBAR_ROW_HEIGHT: f64 = 38.0;
const FOOTER_HEIGHT: f64 = 48.0;
const FORM_ROW_HEIGHT: f64 = 30.0;
const HINT_ROWS: [isize; 5] = [1, 4, 6, 8, 10];

// Navy/indigo palette from the app icon.
const WINDOW_COLOR: u32 = 0x232543;
const FOOTER_COLOR: u32 = 0x1c203a;
const SIDEBAR_COLOR: u32 = 0x1c203a;
const CONTROL_COLOR: u32 = 0x2d3254;
const BORDER_COLOR: u32 = 0x40445e;
/// Silver-blue from the icon's metal. It is light, so text on it is dark.
const ACCENT_COLOR: u32 = 0x999db8;
const ACCENT_TEXT_COLOR: u32 = 0x171925;
const ACCENT_FILL_COLOR: u32 = 0xb3b7d2;
const SECONDARY_TEXT_COLOR: u32 = 0xa3a6bc;
const TERTIARY_TEXT_COLOR: u32 = 0x7c809a;
const AMBER_COLOR: u32 = 0xe2bd6f;

#[derive(Debug)]
struct LauncherState {
    path: PathBuf,
    document: VmConfigDocument,
    defaults: VmConfigDefaults,
}

#[derive(Debug)]
struct LauncherControls {
    grid: Retained<NSGridView>,
    graphics: Retained<NSPopUpButton>,
    graphics_auto: Retained<NSButton>,
    graphics_hint: Retained<NSTextField>,
    get_d3dmetal: Retained<NSStackView>,
    memory_slider: Retained<NSSlider>,
    memory_field: Retained<NSTextField>,
    memory_auto: Retained<NSButton>,
    memory_hint: Retained<NSTextField>,
    cpu_slider: Retained<NSSlider>,
    cpu_field: Retained<NSTextField>,
    cpu_auto: Retained<NSButton>,
    cpu_hint: Retained<NSTextField>,
    disk_field: Retained<NSTextField>,
    disk_auto: Retained<NSButton>,
    disk_hint: Retained<NSTextField>,
    dualsense_passthrough: Retained<NSButton>,
    controllers_hint: Retained<NSTextField>,
    launch: Retained<NSButton>,
}

#[derive(Debug)]
struct AdvancedSheet {
    window: Retained<NSWindow>,
    path: Retained<NSTextField>,
    caption: Retained<NSTextField>,
}

#[derive(Debug, Default)]
struct LauncherDelegateIvars {
    window: RefCell<Option<Retained<NSWindow>>>,
    vm_actions: RefCell<Option<Retained<NSTitlebarAccessoryViewController>>>,
    controls: RefCell<Option<LauncherControls>>,
    state: RefCell<Option<LauncherState>>,
    pending_open: RefCell<Option<PathBuf>>,
    child: RefCell<Option<Child>>,
    log: RefCell<Option<VmLog>>,
    edit_rejected: Cell<bool>,
    sidebar: RefCell<Vec<PathBuf>>,
    advanced: RefCell<Option<AdvancedSheet>>,
    sidebar_visible: Cell<Option<bool>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = LauncherDelegateIvars]
    struct LauncherDelegate;

    unsafe impl NSObjectProtocol for LauncherDelegate {}

    unsafe impl NSMenuItemValidation for LauncherDelegate {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let action = item.action();
            let sheet_open = self
                .ivars()
                .window
                .borrow()
                .as_ref()
                .is_some_and(|window| window.attachedSheet().is_some());
            let idle = self.ivars().child.borrow().is_none() && !sheet_open;
            if action == Some(sel!(newConfig:)) || action == Some(sel!(openConfig:)) {
                return idle.into();
            }
            if action == Some(sel!(diskPresetChosen:)) {
                // Disks only grow, so presets below the current image would have no effect.
                let current = self
                    .ivars()
                    .state
                    .borrow()
                    .as_ref()
                    .and_then(|state| data_disk_size_gib(&state.path, &state.document));
                return (idle && current.is_none_or(|current| item.tag() as u64 >= current)).into();
            }
            if action == Some(sel!(toggleSidebar:)) {
                let visible = self.sidebar_visible();
                item.setTitle(if visible {
                    ns_string!("Hide Sidebar")
                } else {
                    ns_string!("Show Sidebar")
                });
                return (self.ivars().state.borrow().is_some() && (visible || known_vms().len() > 1)).into();
            }
            let Some(path) = self.menu_target(item) else {
                return false.into();
            };
            if action == Some(sel!(removeOrTrashConfig:)) {
                item.setTitle(if is_managed_vm(&path) {
                    ns_string!("Move to Trash…")
                } else {
                    ns_string!("Remove from List")
                });
                // Leave ⌘⌫ to the field editor while a text field is being edited.
                let editing = self
                    .ivars()
                    .window
                    .borrow()
                    .as_ref()
                    .and_then(|window| window.firstResponder())
                    .is_some_and(|responder| responder.isKindOfClass(NSText::class()));
                return (idle && !editing).into();
            }
            idle.into()
        }
    }

    unsafe impl NSControlTextEditingDelegate for LauncherDelegate {
        /// Return in a field commits the value instead of also pressing the default Launch button.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_do_command(&self, control: &NSControl, _text_view: &NSTextView, command: Sel) -> Bool {
            if command != sel!(insertNewline:) {
                return false.into();
            }
            if let Some(window) = control.window() {
                window.makeFirstResponder(None);
            }
            true.into()
        }
    }

    unsafe impl NSTextFieldDelegate for LauncherDelegate {}

    unsafe impl NSApplicationDelegate for LauncherDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, notification: &NSNotification) {
            let app = notification
                .object()
                .and_then(|object| object.downcast::<NSApplication>().ok())
                .expect("application notification must contain NSApplication");
            app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
            app.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).as_deref());
            self.install_main_menu();
            self.create_window();

            let requested = self.ivars().pending_open.borrow_mut().take();
            if let Some(path) = requested
                && let Err(error) = self.show_config(path)
            {
                show_error(self.mtm(), "The virtual machine could not be opened.", &error);
            }
            let opened = self.current_path().is_some();
            if !opened && !known_vms().into_iter().any(|path| self.show_config(path).is_ok()) {
                self.show_empty_state();
            }

            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    0.5,
                    self,
                    sel!(pollChild:),
                    None,
                    true,
                )
            };

            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        }

        #[unsafe(method(application:openFile:))]
        fn open_file(&self, _app: &NSApplication, filename: &NSString) -> Bool {
            let path = PathBuf::from(filename.to_string());
            if self.ivars().window.borrow().is_none() {
                *self.ivars().pending_open.borrow_mut() = Some(path);
                return true.into();
            }
            if self.ivars().child.borrow().is_some() {
                self.activate_vm();
                return false.into();
            }
            match self.show_config(path) {
                Ok(()) => true.into(),
                Err(error) => {
                    show_error(self.mtm(), "The virtual machine could not be opened.", &error);
                    false.into()
                }
            }
        }

        /// Clicking Varmint in the Dock or Finder while a VM runs should bring up that VM, not
        /// the hidden launcher.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, _has_visible_windows: Bool) -> Bool {
            (!self.activate_vm()).into()
        }
    }

    unsafe impl NSWindowDelegate for LauncherDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &NSNotification) {
            self.reload_document();
        }
    }

    impl LauncherDelegate {
        #[unsafe(method(newConfig:))]
        fn new_config(&self, _sender: &AnyObject) {
            if self.ivars().child.borrow().is_some() {
                return;
            }
            let initial = default_vms_directory()
                .map(|directory| unused_vm_name(&directory))
                .unwrap_or_else(|_| "My VM".to_owned());
            self.ask_name("Create Virtual Machine", "Create", &initial, |this, name| {
                match create_vm_config(name) {
                    Ok(path) => {
                        if let Err(error) = this.show_config(path) {
                            show_error(this.mtm(), "The virtual machine could not be opened.", &error);
                        }
                    }
                    Err(error) => show_error(this.mtm(), "The virtual machine could not be created.", &error),
                }
            });
        }

        #[unsafe(method(openConfig:))]
        fn open_config(&self, _sender: &AnyObject) {
            if self.ivars().child.borrow().is_some() {
                return;
            }
            let Some(path) = choose_config(self.mtm()) else { return };
            if let Err(error) = self.show_config(path) {
                show_error(self.mtm(), "The virtual machine could not be opened.", &error);
            }
        }

        #[unsafe(method(showInFinder:))]
        fn show_in_finder(&self, sender: &AnyObject) {
            let Some((path, _)) = self.action_target(sender) else {
                return;
            };
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_slice(&[&*url]));
        }

        #[unsafe(method(renameConfig:))]
        fn rename_config(&self, sender: &AnyObject) {
            let Some((path, document)) = self.action_target(sender) else {
                return;
            };
            let initial = vm_name(&path).to_owned();
            self.ask_name("Rename Virtual Machine", "Rename", &initial, move |this, name| {
                match rename_vm_files(&path, &document, name) {
                    Ok(new_path) => {
                        replace_known_vm(&path, &new_path);
                        if let Err(error) = this.show_config(new_path) {
                            show_error(this.mtm(), "The virtual machine could not be opened.", &error);
                        }
                    }
                    Err(error) => show_error(this.mtm(), "The virtual machine could not be renamed.", &error),
                }
            });
        }

        #[unsafe(method(duplicateConfig:))]
        fn duplicate_config(&self, sender: &AnyObject) {
            let Some((path, document)) = self.action_target(sender) else {
                return;
            };
            let initial = format!("{} copy", vm_name(&path));
            self.ask_name("Duplicate Virtual Machine", "Duplicate", &initial, move |this, name| {
                match duplicate_vm_files(&path, &document, name) {
                    Ok(new_path) => {
                        if let Err(error) = this.show_config(new_path) {
                            show_error(this.mtm(), "The virtual machine could not be opened.", &error);
                        }
                    }
                    Err(error) => show_error(this.mtm(), "The virtual machine could not be duplicated.", &error),
                }
            });
        }

        #[unsafe(method(removeOrTrashConfig:))]
        fn remove_or_trash_config(&self, sender: &AnyObject) {
            let Some((path, document)) = self.action_target(sender) else {
                return;
            };
            if is_managed_vm(&path) {
                let alert = NSAlert::new(self.mtm());
                alert.setMessageText(&NSString::from_str(&format!("Move “{}” to Trash?", vm_name(&path))));
                alert.setInformativeText(if document.storage_dir().is_none() {
                    ns_string!("The configuration and its disks will be moved to Trash.")
                } else {
                    ns_string!("Only the configuration will be moved. Its custom storage directory will remain in place.")
                });
                alert.addButtonWithTitle(ns_string!("Move to Trash"));
                alert.addButtonWithTitle(ns_string!("Cancel"));
                if alert.runModal() != NSAlertFirstButtonReturn {
                    return;
                }
                if let Err(error) = trash_managed_vm(&path, &document) {
                    show_error(self.mtm(), "The virtual machine could not be moved to Trash.", &error);
                    return;
                }
            }
            forget_vm(&path);
            if let Some(next) = first_known_vm() {
                if let Err(error) = self.show_config(next) {
                    show_error(self.mtm(), "The virtual machine could not be opened.", &error);
                    self.show_empty_state();
                }
            } else {
                self.show_empty_state();
            }
        }

        #[unsafe(method(selectKnownVm:))]
        fn select_known_vm(&self, sender: &NSButton) {
            if self.ivars().child.borrow().is_some() {
                return;
            }
            let Some(path) = self.ivars().sidebar.borrow().get(sender.tag() as usize).cloned() else {
                return;
            };
            if let Err(error) = self.show_config(path) {
                show_error(self.mtm(), "The virtual machine could not be opened.", &error);
            }
        }

        #[unsafe(method(toggleSidebar:))]
        fn toggle_sidebar(&self, _sender: &AnyObject) {
            let visible = self.sidebar_visible();
            self.ivars().sidebar_visible.set(Some(!visible));
            let Some(path) = self.current_path() else {
                return;
            };
            if let Err(error) = self.show_config(path) {
                show_error(self.mtm(), "The virtual machine could not be opened.", &error);
            }
        }

        #[unsafe(method(launch:))]
        fn launch(&self, sender: &NSButton) {
            self.ivars().edit_rejected.set(false);
            let Some((path, _)) = self.action_target(sender) else {
                return;
            };
            if self.ivars().edit_rejected.get() {
                return;
            }
            // Only probes the lock; it is released before the VMM takes it.
            let checked = VmConfigDocument::open(&path)
                .and_then(|document| lock_storage(&storage_dir(&path, &document)).map(drop));
            if let Err(error) = checked {
                show_error(self.mtm(), "Varmint could not be launched.", &error);
                return;
            }
            let executable = match std::env::current_exe() {
                Ok(executable) => executable,
                Err(error) => {
                    show_error(self.mtm(), "Varmint could not be launched.", &error.to_string());
                    return;
                }
            };
            match VmLog::spawn(&executable, &path, vm_name(&path)) {
                Ok((child, log)) => {
                    *self.ivars().child.borrow_mut() = Some(child);
                    *self.ivars().log.borrow_mut() = Some(log);
                    self.refresh_controls();
                }
                Err(error) => show_error(self.mtm(), "Varmint could not be launched.", &error.to_string()),
            }
        }

        #[unsafe(method(pollChild:))]
        fn poll_child(&self, _timer: &NSTimer) {
            let result = self.ivars().child.borrow_mut().as_mut().map(Child::try_wait);
            let error = match result {
                None => return,
                Some(Ok(None)) => {
                    self.step_aside_when_vm_is_up();
                    return;
                }
                Some(Ok(Some(status))) if status.success() => None,
                Some(Ok(Some(status))) => {
                    let mut detail = format!("The virtual machine process exited with {status}.");
                    if let Some(log) = self.ivars().log.borrow().as_ref() {
                        detail = format!(
                            "{detail}\n\n{}\n\nFull log: {}",
                            log.last_lines(4),
                            log.path().display()
                        );
                    }
                    Some(detail)
                }
                Some(Err(error)) => Some(error.to_string()),
            };
            self.ivars().child.borrow_mut().take();
            self.reload_document();
            self.refresh_controls();
            self.come_back();
            if let Some(error) = error {
                show_error(self.mtm(), "The virtual machine stopped unexpectedly.", &error);
            }
        }

        #[unsafe(method(graphicsChanged:))]
        fn graphics_changed(&self, sender: &NSPopUpButton) {
            let backend = match sender.indexOfSelectedItem() {
                1 => NeptuneBackend::D3dmetal,
                _ => NeptuneBackend::Dxmt,
            };
            self.edit_document(|document| document.set_neptune_backend(Some(backend)));
        }

        #[unsafe(method(dualSensePassthroughChanged:))]
        fn dualsense_passthrough_changed(&self, sender: &NSButton) {
            let passthrough = sender.state() == NSControlStateValueOn;
            self.edit_document(|document| document.set_dualsense_passthrough(passthrough));
        }

        #[unsafe(method(useAutoGraphics:))]
        fn use_auto_graphics(&self, _sender: &NSButton) {
            self.edit_document(|document| document.set_neptune_backend(None));
        }

        #[unsafe(method(memorySliderChanged:))]
        fn memory_slider_changed(&self, sender: &NSSlider) {
            let memory_mib = sender.doubleValue().round().max(1.0) as u64 * 1024;
            if self.follow_drag(|controls| &controls.memory_field, format_memory(memory_mib)) {
                return;
            }
            self.edit_document(|document| document.set_memory_mib(Some(memory_mib)));
        }

        #[unsafe(method(memoryFieldChanged:))]
        fn memory_field_changed(&self, sender: &NSTextField) {
            let input = sender.stringValue().to_string();
            match parse_memory_mib(&input) {
                Ok(value) => self.edit_document(|document| document.set_memory_mib(value)),
                Err(error) => {
                    self.ivars().edit_rejected.set(true);
                    show_error(self.mtm(), "Memory must be a number of MB or GB.", error);
                    self.refresh_controls();
                }
            }
        }

        #[unsafe(method(useAutoMemory:))]
        fn use_auto_memory(&self, _sender: &NSButton) {
            self.edit_document(|document| document.set_memory_mib(None));
        }

        #[unsafe(method(cpuSliderChanged:))]
        fn cpu_slider_changed(&self, sender: &NSSlider) {
            let vcpus = sender.doubleValue().round().max(1.0) as usize;
            if self.follow_drag(|controls| &controls.cpu_field, vcpus.to_string()) {
                return;
            }
            self.edit_document(|document| document.set_vcpus(Some(vcpus)));
        }

        #[unsafe(method(cpuFieldChanged:))]
        fn cpu_field_changed(&self, sender: &NSTextField) {
            let input = sender.stringValue().to_string();
            match parse_positive_integer(&input) {
                Ok(value) => self.edit_document(|document| document.set_vcpus(value.map(|value| value as usize))),
                Err(error) => {
                    self.ivars().edit_rejected.set(true);
                    show_error(self.mtm(), "CPUs must be a positive whole number.", error);
                    self.refresh_controls();
                }
            }
        }

        #[unsafe(method(useAutoCpu:))]
        fn use_auto_cpu(&self, _sender: &NSButton) {
            self.edit_document(|document| document.set_vcpus(None));
        }

        #[unsafe(method(diskFieldChanged:))]
        fn disk_field_changed(&self, sender: &NSTextField) {
            let input = sender.stringValue().to_string();
            match parse_disk_gib(&input) {
                Ok(value) => self.edit_document(|document| document.set_data_disk_capacity_gib(value)),
                Err(error) => {
                    self.ivars().edit_rejected.set(true);
                    show_error(self.mtm(), "The disk limit must be a number of GB or TB.", error);
                    self.refresh_controls();
                }
            }
        }

        #[unsafe(method(showDiskPresets:))]
        fn show_disk_presets(&self, sender: &NSButton) {
            let mtm = self.mtm();
            let menu = NSMenu::new(mtm);
            for preset in DISK_PRESETS_GIB {
                let item = menu_item(
                    &format_disk(*preset),
                    sel!(diskPresetChosen:),
                    "",
                    NO_MODIFIERS,
                    Some(self),
                    mtm,
                );
                item.setTag(*preset as isize);
                menu.addItem(&item);
            }
            let below = NSPoint::new(0.0, sender.bounds().size.height + 4.0);
            menu.popUpMenuPositioningItem_atLocation_inView(None, below, Some(sender));
        }

        #[unsafe(method(diskPresetChosen:))]
        fn disk_preset_chosen(&self, sender: &NSMenuItem) {
            let capacity_gib = sender.tag() as u64;
            self.edit_document(|document| document.set_data_disk_capacity_gib(Some(capacity_gib)));
        }

        #[unsafe(method(useAutoDisk:))]
        fn use_auto_disk(&self, _sender: &NSButton) {
            self.edit_document(|document| document.set_data_disk_capacity_gib(None));
        }

        #[unsafe(method(showVmActions:))]
        fn show_vm_actions(&self, sender: &NSButton) {
            let below = NSPoint::new(0.0, sender.bounds().size.height + 4.0);
            self.vm_menu(0).popUpMenuPositioningItem_atLocation_inView(None, below, Some(sender));
        }

        #[unsafe(method(showAdvanced:))]
        fn show_advanced(&self, sender: &AnyObject) {
            if self.action_target(sender).is_none() {
                return;
            }
            let Some(window) = self.ivars().window.borrow().clone() else {
                return;
            };
            let sheet = self.advanced_sheet();
            window.beginSheet_completionHandler(&sheet.window, None);
            *self.ivars().advanced.borrow_mut() = Some(sheet);
            self.refresh_advanced();
        }

        #[unsafe(method(closeAdvanced:))]
        fn close_advanced(&self, _sender: &NSButton) {
            let sheet = self.ivars().advanced.borrow().as_ref().map(|sheet| sheet.window.clone());
            let Some(sheet) = sheet else { return };
            sheet.makeFirstResponder(None);
            if let Some(window) = self.ivars().window.borrow().as_ref() {
                window.endSheet(&sheet);
            }
            self.ivars().advanced.borrow_mut().take();
        }

        #[unsafe(method(d3dmetalPathChanged:))]
        fn d3dmetal_path_changed(&self, sender: &NSTextField) {
            let input = sender.stringValue().to_string();
            let input = input.trim();
            if input.is_empty() {
                self.edit_document(|document| document.set_host_environment(D3DMETAL_PATH_VARIABLE, None));
            } else {
                self.use_d3dmetal(&expand_home(input));
            }
        }

        #[unsafe(method(chooseD3DMetal:))]
        fn choose_d3dmetal(&self, _sender: &NSButton) {
            let panel = NSOpenPanel::openPanel(self.mtm());
            panel.setCanChooseDirectories(true);
            panel.setCanChooseFiles(false);
            panel.setTitle(Some(ns_string!("Choose D3DMetal")));
            panel.setMessage(Some(ns_string!(
                "Choose D3DMetal.framework, or the folder or disk image that contains it."
            )));
            if panel.runModal() != NSModalResponseOK {
                return;
            }
            if let Some(chosen) = panel.URL().and_then(|url| url.path()) {
                self.use_d3dmetal(Path::new(&chosen.to_string()));
            }
        }

        #[unsafe(method(downloadD3DMetal:))]
        fn download_d3dmetal(&self, _sender: &NSButton) {
            if let Some(url) = NSURL::URLWithString(&NSString::from_str(D3DMETAL_DOWNLOAD_URL)) {
                NSWorkspace::sharedWorkspace().openURL(&url);
            }
        }
    }
);

define_class!(
    /// Lays out subviews top-down, so sidebar rows stay at the top when the window height changes.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedView {
    fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

impl LauncherDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(LauncherDelegateIvars::default());
        unsafe { msg_send![super(this), init] }
    }

    fn create_window(&self) {
        let mtm = self.mtm();
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, FORM_WIDTH, FORM_HEIGHT),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        window.setBackgroundColor(Some(&color(WINDOW_COLOR, 1.0)));
        window.setTitlebarAppearsTransparent(true);

        let button = symbol_button(
            ns_string!("ellipsis.circle"),
            ns_string!("Virtual Machine Actions"),
            self,
            sel!(showVmActions:),
            mtm,
        );
        button.setFrame(rect(0.0, 0.0, 36.0, 28.0));
        let vm_actions = NSTitlebarAccessoryViewController::new(mtm);
        vm_actions.setView(&button);
        vm_actions.setLayoutAttribute(NSLayoutAttribute::Right);
        window.addTitlebarAccessoryViewController(&vm_actions);

        *self.ivars().window.borrow_mut() = Some(window);
        *self.ivars().vm_actions.borrow_mut() = Some(vm_actions);
    }

    fn show_empty_state(&self) {
        *self.ivars().state.borrow_mut() = None;
        *self.ivars().controls.borrow_mut() = None;

        let mtm = self.mtm();
        let content = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 520.0, 240.0));
        let title = label(
            "No Virtual Machine",
            rect(40.0, 135.0, 440.0, 30.0),
            NSTextAlignment::Center,
            mtm,
        );
        title.setFont(Some(&NSFont::systemFontOfSize_weight(20.0, 0.4)));
        let message = hint_label(
            "Create a virtual machine or open a .varmint configuration.",
            rect(40.0, 105.0, 440.0, 24.0),
            mtm,
        );
        message.setAlignment(NSTextAlignment::Center);
        let open = action_button("Open…", rect(150.0, 55.0, 100.0, 32.0), self, sel!(openConfig:), mtm);
        let create = action_button(
            "Create VM…",
            rect(260.0, 55.0, 110.0, 32.0),
            self,
            sel!(newConfig:),
            mtm,
        );
        create.setKeyEquivalent(ns_string!("\r"));
        create.setBezelColor(Some(&color(ACCENT_COLOR, 1.0)));
        set_prominent_title(&create, "Create VM…");

        content.addSubview(&title);
        content.addSubview(&message);
        content.addSubview(&open);
        content.addSubview(&create);
        self.set_window_content(&content, "Varmint", None);
    }

    fn show_config(&self, path: PathBuf) -> Result<(), String> {
        self.end_editing();
        if path.extension().and_then(|extension| extension.to_str()) != Some("varmint") {
            return Err("Varmint configurations must use the .varmint extension.".to_owned());
        }
        let document = VmConfigDocument::open(&path).map_err(|error| error.to_string())?;
        remember_vm(&path);
        let defaults = VmConfigDefaults::for_host();
        *self.ivars().state.borrow_mut() = Some(LauncherState {
            path: path.clone(),
            document,
            defaults,
        });

        let mtm = self.mtm();
        let known = sidebar_vms();
        *self.ivars().sidebar.borrow_mut() = known.clone();
        if known.len() > 1 && self.ivars().sidebar_visible.get().is_none() {
            self.ivars().sidebar_visible.set(Some(true));
        }
        let sidebar_width = if self.sidebar_visible() { SIDEBAR_WIDTH } else { 0.0 };
        let content = NSView::initWithFrame(
            NSView::alloc(mtm),
            rect(0.0, 0.0, FORM_WIDTH + sidebar_width, FORM_HEIGHT),
        );
        if sidebar_width > 0.0 {
            self.add_sidebar(&content, &known, &path);
        }

        content.addSubview(&filled_box(
            rect(sidebar_width, 0.0, FORM_WIDTH, FOOTER_HEIGHT),
            &color(FOOTER_COLOR, 1.0),
            mtm,
        ));
        content.addSubview(&filled_box(
            rect(sidebar_width, FOOTER_HEIGHT, FORM_WIDTH, 1.0),
            &color(BORDER_COLOR, 0.65),
            mtm,
        ));
        let advanced = action_button(
            "Advanced…",
            rect(sidebar_width + 20.0, 8.0, 105.0, 32.0),
            self,
            sel!(showAdvanced:),
            mtm,
        );
        let launch = action_button(
            "Launch",
            rect(sidebar_width + FORM_WIDTH - 116.0, 8.0, 96.0, 32.0),
            self,
            sel!(launch:),
            mtm,
        );
        launch.setKeyEquivalent(ns_string!("\r"));
        launch.setBezelColor(Some(&color(ACCENT_COLOR, 1.0)));
        set_prominent_title(&launch, "Launch");
        content.addSubview(&advanced);
        content.addSubview(&launch);

        let graphics = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), NSRect::ZERO, false);
        graphics.addItemsWithTitles(&NSArray::from_slice(&[ns_string!("DXMT"), ns_string!("D3DMetal")]));
        unsafe {
            graphics.setTarget(Some(self));
            graphics.setAction(Some(sel!(graphicsChanged:)));
        }
        let graphics_auto = auto_button(self, sel!(useAutoGraphics:), mtm);
        let graphics_hint = hint_label("", NSRect::ZERO, mtm);
        let download = action_button(
            "Download from Apple ↗",
            NSRect::ZERO,
            self,
            sel!(downloadD3DMetal:),
            mtm,
        );
        download.setBordered(false);
        download.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        download.setContentTintColor(Some(&color(ACCENT_FILL_COLOR, 1.0)));
        let locate = action_button("Locate…", NSRect::ZERO, self, sel!(chooseD3DMetal:), mtm);
        locate.setControlSize(NSControlSize::Small);
        locate.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        let get_d3dmetal = NSStackView::stackViewWithViews(
            &NSArray::from_retained_slice(&[into_view(&download), into_view(&locate)]),
            mtm,
        );
        get_d3dmetal.setSpacing(8.0);
        let graphics_caption = NSStackView::stackViewWithViews(
            &NSArray::from_retained_slice(&[into_view(&graphics_hint), into_view(&get_d3dmetal)]),
            mtm,
        );
        graphics_caption.setSpacing(10.0);
        let separator = filled_box(NSRect::ZERO, &color(BORDER_COLOR, 0.65), mtm);
        separator.heightAnchor().constraintEqualToConstant(1.0).setActive(true);

        let memory_slider = slider(
            4.0,
            (defaults.host_memory_mib / 1024).saturating_sub(4).max(4) as f64,
            self,
            sel!(memorySliderChanged:),
            mtm,
        );
        let memory_track = slider_track(&memory_slider, defaults.memory_mib as f64 / 1024.0, mtm);
        let memory_field = input_field(self, sel!(memoryFieldChanged:), mtm);
        let memory_auto = auto_button(self, sel!(useAutoMemory:), mtm);
        let memory_hint = hint_label("", NSRect::ZERO, mtm);

        let host_cpus = defaults.host_cpus;
        let cpu_slider = slider(
            2.0,
            host_cpus.saturating_sub(2).max(2) as f64,
            self,
            sel!(cpuSliderChanged:),
            mtm,
        );
        cpu_slider.setNumberOfTickMarks(host_cpus.saturating_sub(3).max(1) as isize);
        let cpu_track = slider_track(&cpu_slider, defaults.vcpus as f64, mtm);
        let cpu_field = input_field(self, sel!(cpuFieldChanged:), mtm);
        let cpu_auto = auto_button(self, sel!(useAutoCpu:), mtm);
        let cpu_hint = hint_label("", NSRect::ZERO, mtm);

        let disk_field = input_field(self, sel!(diskFieldChanged:), mtm);
        disk_field.setAlignment(NSTextAlignment::Left);
        let disk_presets = symbol_button(
            ns_string!("chevron.down"),
            ns_string!("Disk Sizes"),
            self,
            sel!(showDiskPresets:),
            mtm,
        );
        let disk_auto = auto_button(self, sel!(useAutoDisk:), mtm);
        let disk_hint = hint_label("", NSRect::ZERO, mtm);

        let dualsense_passthrough = unsafe {
            NSButton::checkboxWithTitle_target_action(
                ns_string!("DualSense passthrough"),
                Some(self),
                Some(sel!(dualSensePassthroughChanged:)),
                mtm,
            )
        };
        dualsense_passthrough.setToolTip(Some(ns_string!(
            "Off: a DualSense appears in the VM as an Xbox controller. On: it appears as itself, \
             with touchpad, motion sensors, light bar and adaptive triggers. Other controllers \
             appear as Xbox controllers either way."
        )));
        let controllers_hint = hint_label("", NSRect::ZERO, mtm);

        let empty = || NSGridCell::emptyContentView(mtm);
        let rows: [[Retained<NSView>; 4]; 11] = [
            [
                form_label("DirectX:", mtm),
                into_view(&graphics),
                empty(),
                into_view(&graphics_auto),
            ],
            [empty(), into_view(&graphics_caption), empty(), empty()],
            [empty(), into_view(&separator), empty(), empty()],
            [
                form_label("Memory:", mtm),
                memory_track,
                field_box(&memory_field, 84.0, None, mtm),
                into_view(&memory_auto),
            ],
            [empty(), into_view(&memory_hint), empty(), empty()],
            [
                form_label("CPUs:", mtm),
                cpu_track,
                field_box(&cpu_field, 84.0, None, mtm),
                into_view(&cpu_auto),
            ],
            [empty(), into_view(&cpu_hint), empty(), empty()],
            [
                form_label("Disk limit:", mtm),
                field_box(&disk_field, 118.0, Some(&disk_presets), mtm),
                empty(),
                into_view(&disk_auto),
            ],
            [empty(), into_view(&disk_hint), empty(), empty()],
            [
                form_label("Controllers:", mtm),
                into_view(&dualsense_passthrough),
                empty(),
                empty(),
            ],
            [empty(), into_view(&controllers_hint), empty(), empty()],
        ];
        let rows = rows
            .iter()
            .map(|row| NSArray::from_retained_slice(row))
            .collect::<Vec<_>>();
        let grid = NSGridView::gridViewWithViews(&NSArray::from_retained_slice(&rows), mtm);
        grid.setRowSpacing(0.0);
        grid.setColumnSpacing(10.0);
        grid.setYPlacement(NSGridCellPlacement::Center);
        for (index, width, placement) in [
            (0, 92.0, NSGridCellPlacement::Trailing),
            (1, 316.0, NSGridCellPlacement::Fill),
            (2, 84.0, NSGridCellPlacement::Fill),
            (3, 70.0, NSGridCellPlacement::Leading),
        ] {
            let column = grid.columnAtIndex(index);
            column.setWidth(width);
            column.setXPlacement(placement);
        }
        for index in [0, 3, 5, 7, 9] {
            grid.rowAtIndex(index).setHeight(FORM_ROW_HEIGHT);
        }
        for index in [0, 7, 9] {
            grid.rowAtIndex(index).mergeCellsInRange(NSRange::new(1, 2));
            grid.cellAtColumnIndex_rowIndex(1, index)
                .setXPlacement(NSGridCellPlacement::Leading);
        }
        for index in [1, 2, 4, 6, 8, 10] {
            grid.rowAtIndex(index).mergeCellsInRange(NSRange::new(1, 3));
        }
        for index in HINT_ROWS {
            grid.rowAtIndex(index).setBottomPadding(6.0);
        }
        let separator_row = grid.rowAtIndex(2);
        separator_row.setTopPadding(10.0);
        separator_row.setBottomPadding(10.0);
        grid.setFrameOrigin(NSPoint::new(sidebar_width + 24.0, FOOTER_HEIGHT + 18.0));
        content.addSubview(&grid);

        *self.ivars().controls.borrow_mut() = Some(LauncherControls {
            grid,
            graphics,
            graphics_auto,
            graphics_hint,
            get_d3dmetal,
            memory_slider,
            memory_field,
            memory_auto,
            memory_hint,
            cpu_slider,
            cpu_field,
            cpu_auto,
            cpu_hint,
            disk_field,
            disk_auto,
            disk_hint,
            dualsense_passthrough,
            controllers_hint,
            launch,
        });

        self.refresh_controls();
        self.set_window_content(&content, vm_name(&path), Some(&path));
        Ok(())
    }

    fn add_sidebar(&self, content: &NSView, known: &[PathBuf], selected: &Path) {
        let mtm = self.mtm();
        let height = content.frame().size.height;
        let background = filled_box(rect(0.0, 0.0, SIDEBAR_WIDTH, height), &color(SIDEBAR_COLOR, 1.0), mtm);
        background.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);
        content.addSubview(&background);
        let border = filled_box(
            rect(SIDEBAR_WIDTH - 1.0, 0.0, 1.0, height),
            &color(BORDER_COLOR, 0.65),
            mtm,
        );
        border.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);
        content.addSubview(&border);

        let heading = label(
            "Virtual Machines",
            rect(16.0, height - 34.0, 160.0, 16.0),
            NSTextAlignment::Left,
            mtm,
        );
        heading.setFont(Some(&NSFont::systemFontOfSize_weight(11.0, 0.4)));
        heading.setTextColor(Some(&color(TERTIARY_TEXT_COLOR, 1.0)));
        heading.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
        content.addSubview(&heading);

        let list_height = known.len() as f64 * (SIDEBAR_ROW_HEIGHT + 1.0);
        let list = FlippedView::new(rect(0.0, 0.0, 174.0, list_height), mtm);
        for (index, path) in known.iter().enumerate() {
            let y = index as f64 * (SIDEBAR_ROW_HEIGHT + 1.0);
            if path == selected {
                let selection = filled_box(rect(0.0, y, 174.0, SIDEBAR_ROW_HEIGHT), &color(ACCENT_COLOR, 0.3), mtm);
                selection.setCornerRadius(6.0);
                list.addSubview(&selection);
            }
            let backend = VmConfigDocument::open(path)
                .ok()
                .and_then(|document| document.neptune_backend())
                .unwrap_or_default();
            let name = label(
                vm_name(path),
                rect(8.0, y + 3.0, 158.0, 17.0),
                NSTextAlignment::Left,
                mtm,
            );
            name.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, 0.23)));
            let detail = label(
                backend_name(backend),
                rect(8.0, y + 20.0, 158.0, 14.0),
                NSTextAlignment::Left,
                mtm,
            );
            detail.setFont(Some(&NSFont::systemFontOfSize(11.0)));
            detail.setTextColor(Some(&color(SECONDARY_TEXT_COLOR, 1.0)));
            let button = action_button(
                "",
                rect(0.0, y, 174.0, SIDEBAR_ROW_HEIGHT),
                self,
                sel!(selectKnownVm:),
                mtm,
            );
            button.setBordered(false);
            button.setTag(index as isize);
            unsafe { button.setMenu(Some(&self.vm_menu(index as isize + 1))) };
            list.addSubview(&name);
            list.addSubview(&detail);
            list.addSubview(&button);
        }
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(8.0, 44.0, 174.0, height - 88.0));
        scroll.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);
        scroll.setAutohidesScrollers(true);
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&list));
        content.addSubview(&scroll);

        let add = action_button("+", rect(10.0, 10.0, 28.0, 28.0), self, sel!(newConfig:), mtm);
        add.setBordered(false);
        add.setFont(Some(&NSFont::systemFontOfSize(18.0)));
        content.addSubview(&add);
    }

    /// Sizes the window to the form: empty captions collapse, so the height follows the content.
    fn fit_form(&self) {
        let controls = self.ivars().controls.borrow();
        let Some(controls) = controls.as_ref() else { return };
        let grid = &controls.grid;
        let size = grid.fittingSize();
        let height = FOOTER_HEIGHT + 18.0 + size.height + 20.0;
        let content = unsafe { grid.superview() }.expect("form grid must be in the content view");
        match content.window() {
            Some(window) => {
                let previous = window.frame();
                let mut frame = window.frameRectForContentRect(rect(0.0, 0.0, content.frame().size.width, height));
                frame.origin = NSPoint::new(
                    previous.origin.x,
                    previous.origin.y + previous.size.height - frame.size.height,
                );
                window.setFrame_display(frame, true);
            }
            None => content.setFrameSize(NSSize::new(content.frame().size.width, height)),
        }
        grid.setFrame(NSRect::new(grid.frame().origin, size));
    }

    /// `row` is 0 for the current VM, or a 1-based sidebar row for that row's context menu.
    fn vm_menu(&self, row: isize) -> Retained<NSMenu> {
        let command = NSEventModifierFlags::Command;
        let menu = menu_of(
            "File",
            &[
                Some(("New Virtual Machine…", sel!(newConfig:), "n", command)),
                Some(("Open…", sel!(openConfig:), "o", command)),
                None,
                Some(("Rename…", sel!(renameConfig:), "", NO_MODIFIERS)),
                Some(("Duplicate…", sel!(duplicateConfig:), "", NO_MODIFIERS)),
                Some(("Show in Finder", sel!(showInFinder:), "", NO_MODIFIERS)),
                None,
                Some(("Move to Trash…", sel!(removeOrTrashConfig:), "\u{8}", command)),
            ],
            Some(self),
            self.mtm(),
        );
        for item in menu.itemArray() {
            item.setTag(row);
        }
        menu
    }

    fn install_main_menu(&self) {
        let mtm = self.mtm();
        let command = NSEventModifierFlags::Command;
        let main = NSMenu::new(mtm);
        add_submenu(
            &main,
            &menu_of(
                "Varmint",
                &[
                    Some(("About Varmint", sel!(orderFrontStandardAboutPanel:), "", NO_MODIFIERS)),
                    None,
                    Some(("Hide Varmint", sel!(hide:), "h", command)),
                    Some((
                        "Hide Others",
                        sel!(hideOtherApplications:),
                        "h",
                        command | NSEventModifierFlags::Option,
                    )),
                    Some(("Show All", sel!(unhideAllApplications:), "", NO_MODIFIERS)),
                    None,
                    Some(("Quit Varmint", sel!(terminate:), "q", command)),
                ],
                None,
                mtm,
            ),
        );
        let file = self.vm_menu(0);
        file.addItem(&NSMenuItem::separatorItem(mtm));
        file.addItem(&menu_item("Close", sel!(performClose:), "w", command, None, mtm));
        add_submenu(&main, &file);
        add_submenu(
            &main,
            &menu_of(
                "Edit",
                &[
                    Some(("Undo", sel!(undo:), "z", command)),
                    Some(("Redo", sel!(redo:), "z", command | NSEventModifierFlags::Shift)),
                    None,
                    Some(("Cut", sel!(cut:), "x", command)),
                    Some(("Copy", sel!(copy:), "c", command)),
                    Some(("Paste", sel!(paste:), "v", command)),
                    Some(("Select All", sel!(selectAll:), "a", command)),
                ],
                None,
                mtm,
            ),
        );
        add_submenu(
            &main,
            &menu_of(
                "View",
                &[Some((
                    "Hide Sidebar",
                    sel!(toggleSidebar:),
                    "s",
                    command | NSEventModifierFlags::Control,
                ))],
                Some(self),
                mtm,
            ),
        );
        NSApplication::sharedApplication(mtm).setMainMenu(Some(&main));
    }

    fn menu_target(&self, item: &NSMenuItem) -> Option<PathBuf> {
        match item.tag() {
            0 => self.current_path(),
            row => self.ivars().sidebar.borrow().get(row as usize - 1).cloned(),
        }
    }

    /// Resolves the VM a menu command applies to, switching to a sidebar row first so the
    /// command's dialog and result are shown for that VM.
    fn action_target(&self, sender: &AnyObject) -> Option<(PathBuf, VmConfigDocument)> {
        self.end_editing();
        if self.ivars().child.borrow().is_some() {
            return None;
        }
        if let Some(item) = sender.downcast_ref::<NSMenuItem>() {
            let path = self.menu_target(item)?;
            if self.current_path().as_ref() != Some(&path)
                && let Err(error) = self.show_config(path)
            {
                show_error(self.mtm(), "The virtual machine could not be opened.", &error);
                return None;
            }
        }
        self.ivars()
            .state
            .borrow()
            .as_ref()
            .map(|state| (state.path.clone(), state.document.clone()))
    }

    fn current_path(&self) -> Option<PathBuf> {
        self.ivars().state.borrow().as_ref().map(|state| state.path.clone())
    }

    fn sidebar_visible(&self) -> bool {
        self.ivars().sidebar_visible.get().unwrap_or(false)
    }

    /// Replaces the window's content, keeping its top-left corner where the user left it.
    fn set_window_content(&self, content: &NSView, title: &str, path: Option<&Path>) {
        let window = self
            .ivars()
            .window
            .borrow()
            .clone()
            .expect("launcher window must exist");
        let first_show = !window.isVisible();
        let previous = window.frame();
        let mut frame = window.frameRectForContentRect(content.frame());
        frame.origin = NSPoint::new(
            previous.origin.x,
            previous.origin.y + previous.size.height - frame.size.height,
        );
        window.setContentView(Some(content));
        window.setFrame_display(frame, true);
        window.setTitle(&NSString::from_str(title));
        window.setRepresentedURL(
            path.map(|path| NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy())))
                .as_deref(),
        );
        if let Some(view) = self
            .ivars()
            .vm_actions
            .borrow()
            .as_ref()
            .map(|vm_actions| vm_actions.view())
        {
            view.setHidden(path.is_none());
        }
        if first_show {
            window.center();
        }
        window.makeKeyAndOrderFront(None);
        window.makeFirstResponder(None);
    }

    fn refresh_controls(&self) {
        let state = self.ivars().state.borrow();
        let Some(state) = state.as_ref() else { return };
        let controls = self.ivars().controls.borrow();
        let Some(controls) = controls.as_ref() else { return };
        let idle = self.ivars().child.borrow().is_none();
        for control in [
            AsRef::<NSControl>::as_ref(&controls.graphics),
            controls.memory_slider.as_ref(),
            controls.memory_field.as_ref(),
            controls.cpu_slider.as_ref(),
            controls.cpu_field.as_ref(),
            controls.disk_field.as_ref(),
            controls.dualsense_passthrough.as_ref(),
            controls.launch.as_ref(),
        ] {
            control.setEnabled(idle);
        }
        set_prominent_title(&controls.launch, if idle { "Launch" } else { "Starting…" });

        let explicit_backend = state.document.neptune_backend();
        let backend = explicit_backend.unwrap_or(state.defaults.neptune_backend);
        controls.graphics.selectItemAtIndex(match backend {
            NeptuneBackend::Dxmt => 0,
            NeptuneBackend::D3dmetal => 1,
        });
        set_auto_button(
            &controls.graphics_auto,
            explicit_backend.is_some(),
            idle,
            backend_name(state.defaults.neptune_backend),
        );
        let (hint, warning, offer_d3dmetal) = graphics_hint(backend, &state.document);
        set_hint(&controls.graphics_hint, Some((hint, warning)));
        controls.get_d3dmetal.setHidden(!offer_d3dmetal);

        let explicit_memory = state.document.memory_mib();
        let memory = explicit_memory.unwrap_or(state.defaults.memory_mib);
        controls.memory_slider.setDoubleValue(memory as f64 / 1024.0);
        set_slider_custom(&controls.memory_slider, explicit_memory.is_some());
        set_field_value(
            &controls.memory_field,
            explicit_memory.map(format_memory),
            format_memory(state.defaults.memory_mib),
        );
        set_auto_button(
            &controls.memory_auto,
            explicit_memory.is_some(),
            idle,
            &format_memory(state.defaults.memory_mib),
        );
        set_hint(
            &controls.memory_hint,
            explicit_memory.and_then(|memory| memory_hint(memory, state.defaults.host_memory_mib / 1024)),
        );

        let explicit_vcpus = state.document.vcpus();
        let vcpus = explicit_vcpus.unwrap_or(state.defaults.vcpus);
        controls.cpu_slider.setDoubleValue(vcpus as f64);
        set_slider_custom(&controls.cpu_slider, explicit_vcpus.is_some());
        set_field_value(
            &controls.cpu_field,
            explicit_vcpus.map(|value| value.to_string()),
            state.defaults.vcpus.to_string(),
        );
        set_auto_button(
            &controls.cpu_auto,
            explicit_vcpus.is_some(),
            idle,
            &state.defaults.vcpus.to_string(),
        );
        set_hint(
            &controls.cpu_hint,
            explicit_vcpus.and_then(|vcpus| cpu_hint(vcpus, state.defaults.host_cpus)),
        );

        let explicit_disk = state.document.data_disk_capacity_gib();
        let current_disk = data_disk_size_gib(&state.path, &state.document);
        // Auto never shrinks the disk either, so it means at least the current size.
        let auto_disk = current_disk.map_or(state.defaults.data_disk_capacity_gib, |current| {
            current.max(state.defaults.data_disk_capacity_gib)
        });
        let configured_disk = explicit_disk.unwrap_or(auto_disk);
        set_field_value(
            &controls.disk_field,
            explicit_disk.map(format_disk),
            format_disk(auto_disk),
        );
        set_auto_button(
            &controls.disk_auto,
            explicit_disk.is_some(),
            idle,
            &format_disk(auto_disk),
        );
        set_hint(
            &controls.disk_hint,
            Some((format_disk_hint(configured_disk, current_disk), false)),
        );

        let passthrough = state.document.dualsense_passthrough();
        controls.dualsense_passthrough.setState(passthrough.into());
        set_hint(
            &controls.controllers_hint,
            passthrough.then(|| ("Needs Input Monitoring access.".to_owned(), false)),
        );

        for (row, hint) in HINT_ROWS.into_iter().zip([
            &controls.graphics_hint,
            &controls.memory_hint,
            &controls.cpu_hint,
            &controls.disk_hint,
            &controls.controllers_hint,
        ]) {
            controls
                .grid
                .rowAtIndex(row)
                .setHidden(hint.stringValue().length() == 0);
        }
        self.fit_form();
        self.refresh_advanced();
    }

    /// Applies one edit to the file as it is on disk now, so edits made in a text editor while
    /// the launcher is open are kept.
    fn edit_document(&self, edit: impl FnOnce(&mut VmConfigDocument)) {
        if self.ivars().child.borrow().is_some() {
            return;
        }
        let Some(path) = self.current_path() else { return };
        let result = VmConfigDocument::open(&path)
            .map_err(|error| error.to_string())
            .and_then(|mut document| {
                let before = document.to_string();
                edit(&mut document);
                if document.to_string() != before {
                    document.save(&path).map_err(|error| error.to_string())?;
                }
                Ok(document)
            });
        match result {
            Ok(document) => {
                if let Some(state) = self.ivars().state.borrow_mut().as_mut() {
                    state.document = document;
                }
            }
            Err(error) => show_error(
                self.mtm(),
                "The virtual machine configuration could not be saved.",
                &error,
            ),
        }
        self.refresh_controls();
    }

    fn reload_document(&self) {
        let Some(path) = self.current_path() else { return };
        let Ok(document) = VmConfigDocument::open(&path) else {
            return;
        };
        let changed = self.ivars().state.borrow_mut().as_mut().is_some_and(|state| {
            let changed = state.document.to_string() != document.to_string();
            state.document = document;
            changed
        });
        if changed {
            self.refresh_controls();
        }
    }

    /// While the VM runs, the launcher leaves the Dock and hides its window, so Varmint shows as
    /// one app: the VM. Settings can't be edited during a run anyway.
    ///
    /// The launcher waits until the VM process has finished launching as an app: before that the
    /// VMM may spend a while unpacking the system image with no window, and the launcher shows
    /// "Starting…" meanwhile.
    fn step_aside_when_vm_is_up(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        if app.activationPolicy() != NSApplicationActivationPolicy::Regular {
            return;
        }
        let pid = self.ivars().child.borrow().as_ref().map(Child::id);
        let vm = pid.and_then(|pid| NSRunningApplication::runningApplicationWithProcessIdentifier(pid as libc::pid_t));
        if !vm.is_some_and(|vm| vm.isFinishedLaunching()) {
            return;
        }
        if let Some(window) = self.ivars().window.borrow().as_ref() {
            window.orderOut(None);
        }
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }

    fn come_back(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        if let Some(window) = self.ivars().window.borrow().as_ref() {
            window.makeKeyAndOrderFront(None);
        }
    }

    /// Asks for a VM name in a sheet on the launcher window and continues with `then` once the
    /// user confirms.
    fn ask_name(&self, title: &str, action: &str, initial: &str, then: impl Fn(&LauncherDelegate, &str) + 'static) {
        let mtm = self.mtm();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(ns_string!("Choose a name for the virtual machine."));
        alert.addButtonWithTitle(&NSString::from_str(action));
        alert.addButtonWithTitle(ns_string!("Cancel"));
        let field = NSTextField::initWithFrame(NSTextField::alloc(mtm), rect(0.0, 0.0, 280.0, 24.0));
        field.setStringValue(&NSString::from_str(initial));
        alert.setAccessoryView(Some(&field));
        alert.window().setInitialFirstResponder(Some(&field));
        self.show_sheet(&alert, move |this, response| {
            if response == NSAlertFirstButtonReturn {
                then(this, &field.stringValue().to_string());
            }
        });
    }

    /// Shows the alert as a sheet on the launcher window; `then` gets the button the user chose.
    fn show_sheet(&self, alert: &NSAlert, then: impl Fn(&LauncherDelegate, NSModalResponse) + 'static) {
        let Some(window) = self.ivars().window.borrow().clone() else {
            return;
        };
        let this = self.retain();
        let handler = RcBlock::new(move |response: NSModalResponse| then(&this, response));
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
    }

    fn use_d3dmetal(&self, chosen: &Path) {
        match find_d3dmetal(chosen) {
            Some(framework) => self.edit_document(|document| {
                document.set_host_environment(D3DMETAL_PATH_VARIABLE, Some(&framework.to_string_lossy()))
            }),
            None => {
                show_error(
                    self.mtm(),
                    "D3DMetal wasn't found there.",
                    "Choose D3DMetal.framework. libd3dshared.dylib must be next to it.",
                );
                self.refresh_advanced();
            }
        }
    }

    fn advanced_sheet(&self) -> AdvancedSheet {
        let mtm = self.mtm();
        let width = 540.0;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, width, 112.0),
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setBackgroundColor(Some(&color(WINDOW_COLOR, 1.0)));
        let content = window.contentView().expect("window must have a content view");
        // Sheets draw their own material instead of the window background color.
        let background = filled_box(content.bounds(), &color(WINDOW_COLOR, 1.0), mtm);
        background.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.addSubview(&background);

        let title = label("D3DMetal:", rect(20.0, 72.0, 92.0, 17.0), NSTextAlignment::Right, mtm);
        title.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        let path = input_field(self, sel!(d3dmetalPathChanged:), mtm);
        path.setAlignment(NSTextAlignment::Left);
        if let Some(cell) = path.cell() {
            cell.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        }
        let path_box = field_box(&path, 300.0, None, mtm);
        path_box.setFrameOrigin(NSPoint::new(122.0, 68.0));
        let choose = action_button(
            "Choose…",
            rect(430.0, 64.0, 92.0, 32.0),
            self,
            sel!(chooseD3DMetal:),
            mtm,
        );
        let caption = hint_label("", rect(122.0, 46.0, 400.0, 16.0), mtm);
        let done = action_button(
            "Done",
            rect(width - 112.0, 12.0, 92.0, 32.0),
            self,
            sel!(closeAdvanced:),
            mtm,
        );
        done.setKeyEquivalent(ns_string!("\r"));
        done.setBezelColor(Some(&color(ACCENT_COLOR, 1.0)));
        set_prominent_title(&done, "Done");
        for view in [
            title.as_ref(),
            path_box.as_ref(),
            choose.as_ref(),
            caption.as_ref(),
            done.as_ref(),
        ] {
            content.addSubview(view);
        }
        window.setInitialFirstResponder(Some(&done));
        AdvancedSheet { window, path, caption }
    }

    /// Shows the VM's D3DMetal choice in the Advanced sheet. An empty field means the bundled
    /// D3DMetal, like Auto in the main form.
    fn refresh_advanced(&self) {
        let advanced = self.ivars().advanced.borrow();
        let state = self.ivars().state.borrow();
        let (Some(sheet), Some(state)) = (advanced.as_ref(), state.as_ref()) else {
            return;
        };
        let chosen = state.document.host_environment(D3DMETAL_PATH_VARIABLE).map(Path::new);
        let bundled = bundled_d3dmetal().and_then(|path| d3dmetal_version(&path));
        set_field_value(
            &sheet.path,
            chosen.map(abbreviate_home),
            bundled.map_or_else(|| "Not included".to_owned(), |version| format!("Bundled {version}")),
        );
        let hint = chosen.map(|path| match d3dmetal_version(path) {
            Some(version) => (format!("D3DMetal {version}"), false),
            None => ("D3DMetal wasn't found there.".to_owned(), true),
        });
        set_hint(&sheet.caption, hint);
    }

    /// Commits a field that is being edited, so clicking Launch, a sidebar row or a menu command
    /// doesn't drop what was typed.
    fn end_editing(&self) {
        let window = self.ivars().window.borrow().clone();
        if let Some(window) = window {
            window.makeFirstResponder(None);
        }
    }

    /// While a slider is dragged only its field follows; the file is written once, on release.
    fn follow_drag(&self, field: impl FnOnce(&LauncherControls) -> &Retained<NSTextField>, text: String) -> bool {
        let dragging = NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .is_some_and(|event| event.r#type() == NSEventType::LeftMouseDragged);
        if dragging && let Some(controls) = self.ivars().controls.borrow().as_ref() {
            field(controls).setStringValue(&NSString::from_str(&text));
        }
        dragging
    }

    /// Brings the running VM to the front. Returns false when there is no VM app to show yet.
    fn activate_vm(&self) -> bool {
        let pid = self.ivars().child.borrow().as_ref().map(Child::id);
        let Some(vm) =
            pid.and_then(|pid| NSRunningApplication::runningApplicationWithProcessIdentifier(pid as libc::pid_t))
        else {
            return false;
        };
        NSApplication::sharedApplication(self.mtm()).yieldActivationToApplication(&vm);
        vm.activateWithOptions(NSApplicationActivationOptions::empty());
        true
    }
}

pub fn run() {
    let mtm = MainThreadMarker::new().expect("Varmint GUI must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let delegate = LauncherDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn label(text: &str, frame: NSRect, alignment: NSTextAlignment, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(frame);
    label.setAlignment(alignment);
    label
}

fn hint_label(text: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = label(text, frame, NSTextAlignment::Left, mtm);
    label.setTextColor(Some(&color(SECONDARY_TEXT_COLOR, 1.0)));
    label.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    label
}

fn input_field(target: &LauncherDelegate, action: Sel, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(ns_string!(""), mtm);
    if let Some(cell) = field.cell() {
        cell.setSendsActionOnEndEditing(true);
    }
    unsafe { field.setDelegate(Some(ProtocolObject::from_ref(target))) };
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setAlignment(NSTextAlignment::Right);
    field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    unsafe {
        field.setTarget(Some(target));
        field.setAction(Some(action));
    }
    field
}

/// A borderless field inside a tinted rounded box. Bezeled fields draw an opaque near-black
/// fill that ignores the background color, which clashes with the navy window.
fn field_box(field: &NSTextField, width: f64, accessory: Option<&NSView>, mtm: MainThreadMarker) -> Retained<NSView> {
    let height = 24.0;
    let frame = filled_box(rect(0.0, 0.0, width, height), &color(CONTROL_COLOR, 1.0), mtm);
    frame.setCornerRadius(5.0);
    frame.setBorderWidth(1.0);
    frame.setBorderColor(&color(BORDER_COLOR, 1.0));
    frame.setContentViewMargins(NSSize::ZERO);
    frame.widthAnchor().constraintEqualToConstant(width).setActive(true);
    frame.heightAnchor().constraintEqualToConstant(height).setActive(true);
    let accessory_width = accessory.map_or(0.0, |accessory| {
        accessory.setFrame(rect(width - height, 0.0, height, height));
        frame.addSubview(accessory);
        height
    });
    field.setFrame(rect(6.0, 3.5, width - 12.0 - accessory_width, 17.0));
    frame.addSubview(field);
    into_view(&frame)
}

fn symbol_button(
    symbol: &NSString,
    description: &NSString,
    target: &LauncherDelegate,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(symbol, Some(description))
        .expect("SF Symbol must exist");
    let button = unsafe { NSButton::buttonWithImage_target_action(&image, Some(target), Some(action), mtm) };
    button.setBordered(false);
    button
}

fn slider(min: f64, max: f64, target: &LauncherDelegate, action: Sel, mtm: MainThreadMarker) -> Retained<NSSlider> {
    let slider = NSSlider::initWithFrame(NSSlider::alloc(mtm), rect(0.0, 6.0, 316.0, 24.0));
    slider.setMinValue(min);
    slider.setMaxValue(max);
    slider.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    unsafe {
        slider.setTarget(Some(target));
        slider.setAction(Some(action));
    }
    slider
}

/// The slider plus a short mark under its track at the Auto value. Moving the knob onto the
/// mark does not return the setting to Auto.
fn slider_track(slider: &NSSlider, auto_value: f64, mtm: MainThreadMarker) -> Retained<NSView> {
    let track = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 316.0, FORM_ROW_HEIGHT));
    track
        .heightAnchor()
        .constraintEqualToConstant(FORM_ROW_HEIGHT)
        .setActive(true);
    let frame = slider.frame();
    let knob = slider.knobThickness();
    let range = slider.maxValue() - slider.minValue();
    let fraction = if range > 0.0 {
        ((auto_value - slider.minValue()) / range).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x = frame.origin.x + knob / 2.0 + fraction * (frame.size.width - knob);
    track.addSubview(slider);
    track.addSubview(&filled_box(rect(x - 1.0, 1.0, 2.0, 5.0), &color(0xffffff, 0.6), mtm));
    track
}

fn filled_box(frame: NSRect, fill: &NSColor, mtm: MainThreadMarker) -> Retained<NSBox> {
    let filled = NSBox::initWithFrame(NSBox::alloc(mtm), frame);
    filled.setBoxType(NSBoxType::Custom);
    filled.setBorderWidth(0.0);
    filled.setFillColor(fill);
    filled
}

fn color(rgb: u32, alpha: f64) -> Retained<NSColor> {
    let channel = |shift: u32| ((rgb >> shift) & 0xff) as f64 / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(channel(16), channel(8), channel(0), alpha)
}

fn form_label(text: &str, mtm: MainThreadMarker) -> Retained<NSView> {
    let label = label(text, NSRect::ZERO, NSTextAlignment::Right, mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    into_view(&*label)
}

fn into_view(view: &NSView) -> Retained<NSView> {
    view.retain()
}

fn action_button(
    title: &str,
    frame: NSRect,
    target: &LauncherDelegate,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button =
        unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str(title), Some(target), Some(action), mtm) };
    button.setFrame(frame);
    button
}

fn auto_button(target: &LauncherDelegate, action: Sel, mtm: MainThreadMarker) -> Retained<NSButton> {
    let button = action_button("Auto", NSRect::ZERO, target, action, mtm);
    button.setControlSize(NSControlSize::Small);
    button.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    button
}

const NO_MODIFIERS: NSEventModifierFlags = NSEventModifierFlags(0);

/// A menu entry: title, action, key equivalent, modifiers; `None` is a separator.
type MenuEntry = Option<(&'static str, Sel, &'static str, NSEventModifierFlags)>;

/// Items without a target go to the first responder, which covers the standard app and edit
/// commands.
fn menu_of(
    title: &str,
    entries: &[MenuEntry],
    target: Option<&LauncherDelegate>,
    mtm: MainThreadMarker,
) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for entry in entries {
        match entry {
            Some((title, action, key, modifiers)) => {
                menu.addItem(&menu_item(title, *action, key, *modifiers, target, mtm));
            }
            None => menu.addItem(&NSMenuItem::separatorItem(mtm)),
        }
    }
    menu
}

fn menu_item(
    title: &str,
    action: Sel,
    key: &str,
    modifiers: NSEventModifierFlags,
    target: Option<&LauncherDelegate>,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    item.setKeyEquivalentModifierMask(modifiers);
    unsafe { item.setTarget(target.map(|target| target as &AnyObject)) };
    item
}

fn add_submenu(main: &NSMenu, submenu: &NSMenu) {
    let item = NSMenuItem::new(main.mtm());
    item.setSubmenu(Some(submenu));
    main.addItem(&item);
}

fn set_field_value(field: &NSTextField, explicit: Option<String>, automatic: String) {
    field.setStringValue(&NSString::from_str(explicit.as_deref().unwrap_or("")));
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setAlignment(field.alignment());
    let font = field.font().expect("input fields have a font");
    let gray = color(TERTIARY_TEXT_COLOR, 1.0);
    let attributes = unsafe {
        NSDictionary::from_slices(
            &[
                NSForegroundColorAttributeName,
                NSFontAttributeName,
                NSParagraphStyleAttributeName,
            ],
            &[&*gray as &AnyObject, &*font, &*paragraph],
        )
    };
    let placeholder = unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(&automatic), &attributes) };
    field.setPlaceholderAttributedString(Some(&placeholder));
}

/// Custom values show a small "Use Auto" button; Auto values show plain, unclickable "Auto" text.
fn set_prominent_title(button: &NSButton, title: &str) {
    let text = color(ACCENT_TEXT_COLOR, 1.0);
    let font = NSFont::systemFontOfSize_weight(13.0, 0.3);
    let attributes = unsafe {
        NSDictionary::from_slices(
            &[NSForegroundColorAttributeName, NSFontAttributeName],
            &[&*text as &AnyObject, &*font],
        )
    };
    let title = unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(title), &attributes) };
    button.setAttributedTitle(&title);
}

fn set_auto_button(button: &NSButton, explicit: bool, enabled: bool, automatic: &str) {
    button.setBordered(explicit);
    button.setEnabled(explicit && enabled);
    button.setTitle(if explicit {
        ns_string!("Use Auto")
    } else {
        ns_string!("Auto")
    });
    button.setToolTip(
        explicit
            .then(|| NSString::from_str(&format!("Auto is {automatic}")))
            .as_deref(),
    );
}

fn set_slider_custom(slider: &NSSlider, explicit: bool) {
    let fill = if explicit {
        color(ACCENT_FILL_COLOR, 1.0)
    } else {
        color(0xffffff, 0.34)
    };
    slider.setTrackFillColor(Some(&fill));
}

fn set_hint(label: &NSTextField, hint: Option<(String, bool)>) {
    let (text, warning) = hint.unwrap_or_default();
    label.setStringValue(&NSString::from_str(&text));
    label.setTextColor(Some(&color(
        if warning { AMBER_COLOR } else { SECONDARY_TEXT_COLOR },
        1.0,
    )));
}

fn choose_config(mtm: MainThreadMarker) -> Option<PathBuf> {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(false);
    panel.setCanChooseFiles(true);
    panel.setAllowsMultipleSelection(false);
    panel.setTitle(Some(ns_string!("Open Virtual Machine")));
    panel.setMessage(Some(ns_string!("Choose a Varmint configuration.")));
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    let path = panel.URL()?.path()?.to_string();
    Some(PathBuf::from(path))
}

fn backend_name(backend: NeptuneBackend) -> &'static str {
    match backend {
        NeptuneBackend::Dxmt => "DXMT",
        NeptuneBackend::D3dmetal => "D3DMetal",
    }
}

/// The D3DMetal framework the VMM would load, and whether it is the copy bundled with Varmint.
/// Like the VMM, this prefers `D3DMETAL_FRAMEWORK_PATH` from the environment, then from the
/// config's `host_environment`, then the bundle.
fn d3dmetal_framework(document: &VmConfigDocument) -> Option<(PathBuf, bool)> {
    let chosen = std::env::var_os(D3DMETAL_PATH_VARIABLE)
        .map(PathBuf::from)
        .or_else(|| document.host_environment(D3DMETAL_PATH_VARIABLE).map(PathBuf::from));
    if let Some(path) = chosen {
        return Some((path, false));
    }
    bundled_d3dmetal().map(|path| (path, true))
}

/// The D3DMetal framework in the app bundle, if the bundle can use it.
fn bundled_d3dmetal() -> Option<PathBuf> {
    let contents = std::env::current_exe().ok()?.parent()?.parent()?.to_owned();
    let bundled = contents.join("Frameworks/D3DMetal/D3DMetal.framework");
    (bundled.is_dir() && contents.join("Frameworks/libd3dmetal-native.dylib").is_file()).then_some(bundled)
}

/// The display version of a D3DMetal framework, or `None` if `path` isn't one.
fn d3dmetal_version(path: &Path) -> Option<String> {
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(&path.to_string_lossy()))?;
    if bundle.bundleIdentifier()?.to_string() != "com.apple.D3DMetal" {
        return None;
    }
    let version = bundle.objectForInfoDictionaryKey(ns_string!("CFBundleShortVersionString"))?;
    Some(display_version(&version.downcast::<NSString>().ok()?.to_string()))
}

/// Finds D3DMetal at a location the user chose: the framework itself, its folder, or the root
/// of Apple's disk image.
fn find_d3dmetal(chosen: &Path) -> Option<PathBuf> {
    [
        chosen.to_owned(),
        chosen.join("D3DMetal.framework"),
        chosen.join("redist/lib/external/D3DMetal.framework"),
    ]
    .into_iter()
    .find(|framework| {
        d3dmetal_version(framework).is_some()
            && framework
                .parent()
                .is_some_and(|folder| folder.join("libd3dshared.dylib").is_file())
    })
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::home_dir()) {
        (Some(relative), Some(home)) => home.join(relative),
        _ => PathBuf::from(path),
    }
}

fn abbreviate_home(path: &Path) -> String {
    match std::env::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_owned)) {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

/// "4.0b3" -> "4.0 beta 3".
fn display_version(version: &str) -> String {
    match version.split_once('b') {
        Some((release, beta)) if !beta.is_empty() && beta.bytes().all(|byte| byte.is_ascii_digit()) => {
            format!("{release} beta {beta}")
        }
        _ => version.to_owned(),
    }
}

/// The caption under Graphics, whether it is a warning, and whether to offer getting D3DMetal.
fn graphics_hint(backend: NeptuneBackend, document: &VmConfigDocument) -> (String, bool, bool) {
    if backend == NeptuneBackend::Dxmt {
        return ("Bundled.".to_owned(), false, false);
    }
    match d3dmetal_framework(document) {
        Some((path, bundled)) => match d3dmetal_version(&path) {
            Some(version) if bundled => (format!("D3DMetal {version} · Bundled."), false, false),
            Some(version) => (format!("D3DMetal {version} · Chosen in Advanced."), false, false),
            None => ("D3DMetal wasn't found where it was chosen.".to_owned(), true, true),
        },
        None => (
            "D3DMetal isn't included in this build of Varmint.".to_owned(),
            true,
            true,
        ),
    }
}

fn show_error(mtm: MainThreadMarker, message: &str, detail: &str) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(message));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.addButtonWithTitle(ns_string!("OK"));
    alert.runModal();
}

fn format_memory(memory_mib: u64) -> String {
    if memory_mib % 1024 == 0 {
        format!("{} GB", memory_mib / 1024)
    } else {
        format!("{memory_mib} MB")
    }
}

fn format_disk(capacity_gib: u64) -> String {
    if capacity_gib % 1024 == 0 {
        format!("{} TB", capacity_gib / 1024)
    } else {
        format!("{capacity_gib} GB")
    }
}

fn memory_hint(memory_mib: u64, host_memory_gib: u64) -> Option<(String, bool)> {
    if memory_mib > host_memory_gib * 1024 {
        return Some((format!("More than this Mac's {host_memory_gib} GB."), true));
    }
    if memory_mib > host_memory_gib.saturating_sub(4) * 1024 {
        return Some(("Leaves less than 4 GB for macOS.".to_owned(), false));
    }
    if memory_mib < 4 * 1024 {
        return Some(("Below the 4 GB slider range.".to_owned(), false));
    }
    None
}

fn cpu_hint(vcpus: usize, host_cpus: usize) -> Option<(String, bool)> {
    if vcpus > host_cpus {
        return Some((format!("This Mac has {host_cpus} CPUs."), true));
    }
    if vcpus > host_cpus.saturating_sub(2) {
        return Some(("Leaves fewer than 2 CPUs for macOS.".to_owned(), false));
    }
    if vcpus < 2 {
        return Some(("Below the 2 CPU slider range.".to_owned(), false));
    }
    None
}

fn data_disk_size_gib(config_path: &Path, document: &VmConfigDocument) -> Option<u64> {
    let bytes = storage_dir(config_path, document)
        .join("data.raw")
        .metadata()
        .ok()?
        .len();
    Some(bytes.saturating_add(GIB - 1) / GIB)
}

fn format_disk_hint(configured_gib: u64, current_gib: Option<u64>) -> String {
    let Some(current_gib) = current_gib else {
        return format!(
            "Up to {}, using space only as the data is written.",
            format_disk(configured_gib)
        );
    };
    if configured_gib == current_gib {
        return format!(
            "Up to {}, using space only as the data is written. Can't be lowered.",
            format_disk(current_gib)
        );
    }
    if configured_gib > current_gib {
        return format!(
            "Now {}, {} from the next launch. Can't be lowered after that.",
            format_disk(current_gib),
            format_disk(configured_gib)
        );
    }
    format!(
        "Already {} and can't be lowered, so {} has no effect.",
        format_disk(current_gib),
        format_disk(configured_gib)
    )
}

fn parse_memory_mib(input: &str) -> Result<Option<u64>, &'static str> {
    let value = parse_size(input, 1024, 1024, "mb", "gb")?;
    if value.is_some_and(|value| value < 256) {
        return Err("Memory must be at least 256 MB.");
    }
    Ok(value)
}

fn parse_disk_gib(input: &str) -> Result<Option<u64>, &'static str> {
    parse_size(input, 1024, 1, "gb", "tb")
}

fn parse_size(
    input: &str,
    large_multiplier: u64,
    default_multiplier: u64,
    small_unit: &str,
    large_unit: &str,
) -> Result<Option<u64>, &'static str> {
    let input = input.trim().to_ascii_lowercase();
    if input.is_empty() {
        return Ok(None);
    }
    let (number, multiplier) = if let Some(number) = input.strip_suffix(large_unit) {
        (number.trim(), large_multiplier)
    } else if let Some(number) = input.strip_suffix(small_unit) {
        (number.trim(), 1)
    } else {
        (input.as_str(), default_multiplier)
    };
    let value = number
        .parse::<u64>()
        .map_err(|_| "Enter a positive whole number with an optional unit.")?;
    value
        .checked_mul(multiplier)
        .filter(|value| *value > 0 && i64::try_from(*value).is_ok())
        .map(Some)
        .ok_or("The value is too large or zero.")
}

fn parse_positive_integer(input: &str) -> Result<Option<u64>, &'static str> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }
    input
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0 && i64::try_from(*value).is_ok())
        .map(Some)
        .ok_or("Enter a positive whole number.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_memory_units() {
        assert_eq!(parse_memory_mib("24").unwrap(), Some(24 * 1024));
        assert_eq!(parse_memory_mib("24 GB").unwrap(), Some(24 * 1024));
        assert_eq!(parse_memory_mib("20000 MB").unwrap(), Some(20000));
        assert_eq!(parse_memory_mib("").unwrap(), None);
        assert_eq!(parse_memory_mib("255 MB"), Err("Memory must be at least 256 MB."));
    }

    #[test]
    fn parses_disk_units() {
        assert_eq!(parse_disk_gib("256").unwrap(), Some(256));
        assert_eq!(parse_disk_gib("256 GB").unwrap(), Some(256));
        assert_eq!(parse_disk_gib("1 TB").unwrap(), Some(1024));
        assert_eq!(parse_disk_gib("").unwrap(), None);
    }

    #[test]
    fn describes_data_disk_growth() {
        assert_eq!(
            format_disk_hint(256, None),
            "Up to 256 GB, using space only as the data is written."
        );
        assert_eq!(
            format_disk_hint(256, Some(256)),
            "Up to 256 GB, using space only as the data is written. Can't be lowered."
        );
        assert_eq!(
            format_disk_hint(256, Some(128)),
            "Now 128 GB, 256 GB from the next launch. Can't be lowered after that."
        );
        assert_eq!(
            format_disk_hint(96, Some(256)),
            "Already 256 GB and can't be lowered, so 96 GB has no effect."
        );
    }

    #[test]
    fn describes_values_outside_resource_slider_ranges() {
        assert_eq!(
            memory_hint(40 * 1024, 36),
            Some(("More than this Mac's 36 GB.".to_owned(), true))
        );
        assert_eq!(
            memory_hint(34 * 1024, 36),
            Some(("Leaves less than 4 GB for macOS.".to_owned(), false))
        );
        assert_eq!(cpu_hint(13, 12), Some(("This Mac has 12 CPUs.".to_owned(), true)));
        assert_eq!(
            cpu_hint(11, 12),
            Some(("Leaves fewer than 2 CPUs for macOS.".to_owned(), false))
        );
    }

    #[test]
    fn rejects_values_too_large_for_toml() {
        assert!(parse_memory_mib("18446744073709551615 MB").is_err());
        assert!(parse_disk_gib("9223372036854775807 TB").is_err());
        assert!(parse_positive_integer("9223372036854775808").is_err());
    }

    #[test]
    fn displays_beta_versions() {
        assert_eq!(display_version("4.0b3"), "4.0 beta 3");
        assert_eq!(display_version("4.0"), "4.0");
    }
}
