use objc2::{
    class, msg_send,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use objc2_foundation::NSRange;
use std::{
    ffi::c_char,
    ptr,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

const COMMAND: usize = 1 << 20;
const SHIFT: usize = 1 << 17;
const CONTROL: usize = 1 << 18;

const NS_UTF8_STRING_ENCODING: usize = 4;

static MENU_INSTALLED: AtomicBool = AtomicBool::new(false);
static TOGGLE_STATISTICS: AtomicBool = AtomicBool::new(false);
static SHOW_ABOUT: AtomicBool = AtomicBool::new(false);
static STATISTICS_ITEM: AtomicPtr<AnyObject> = AtomicPtr::new(ptr::null_mut());

unsafe extern "C-unwind" fn toggle_statistics_action(
    _receiver: *mut AnyObject,
    _selector: Sel,
    _sender: *mut AnyObject,
) {
    TOGGLE_STATISTICS.store(true, Ordering::Release);
}

unsafe extern "C-unwind" fn show_about_action(_receiver: *mut AnyObject, _selector: Sel, _sender: *mut AnyObject) {
    SHOW_ABOUT.store(true, Ordering::Release);
}

unsafe fn install_action(
    selector: Sel,
    implementation: unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
) {
    unsafe {
        let class = class!(NSApplication) as *const AnyClass as *mut AnyClass;
        let implementation: Imp = std::mem::transmute::<
            unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
            Imp,
        >(implementation);
        let types = b"v@:@\0";
        let _ = objc2::ffi::class_addMethod(class, selector, implementation, types.as_ptr().cast::<c_char>());
    }
}

unsafe fn ns_string(value: &str) -> *mut AnyObject {
    unsafe {
        let string: *mut AnyObject = msg_send![class!(NSString), alloc];
        let string: *mut AnyObject = msg_send![
            string,
            initWithBytes: value.as_ptr(),
            length: value.len(),
            encoding: NS_UTF8_STRING_ENCODING
        ];
        string
    }
}

unsafe fn new_menu(title: &str) -> *mut AnyObject {
    unsafe {
        let title = ns_string(title);
        let menu: *mut AnyObject = msg_send![class!(NSMenu), alloc];
        let menu: *mut AnyObject = msg_send![menu, initWithTitle: title];
        objc2::ffi::objc_release(title);
        menu
    }
}

unsafe fn new_menu_item(
    title: &str,
    action: Option<Sel>,
    key: &str,
    modifiers: usize,
    target: *mut AnyObject,
) -> *mut AnyObject {
    unsafe {
        let title = ns_string(title);
        let key = ns_string(key);
        let item: *mut AnyObject = msg_send![class!(NSMenuItem), alloc];
        let item: *mut AnyObject = msg_send![
            item,
            initWithTitle: title,
            action: action,
            keyEquivalent: key
        ];
        objc2::ffi::objc_release(title);
        objc2::ffi::objc_release(key);
        let _: () = msg_send![item, setTarget: target];
        let _: () = msg_send![item, setKeyEquivalentModifierMask: modifiers];
        item
    }
}

unsafe fn add_item(menu: *mut AnyObject, item: *mut AnyObject) {
    let _: () = msg_send![menu, addItem: item];
}

unsafe fn add_separator(menu: *mut AnyObject) {
    unsafe {
        let separator: *mut AnyObject = msg_send![class!(NSMenuItem), separatorItem];
        add_item(menu, separator);
    }
}

unsafe fn add_submenu(main_menu: *mut AnyObject, title: &str, submenu: *mut AnyObject) -> *mut AnyObject {
    unsafe {
        let item = new_menu_item(title, None, "", 0, ptr::null_mut());
        let _: () = msg_send![item, setSubmenu: submenu];
        add_item(main_menu, item);
        item
    }
}

pub fn install_main_menu() {
    if MENU_INSTALLED.swap(true, Ordering::AcqRel) {
        return;
    }

    unsafe {
        install_action(sel!(varmintToggleStatistics:), toggle_statistics_action);
        install_action(sel!(varmintShowAbout:), show_about_action);

        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let main_menu = new_menu("");

        let app_menu = new_menu("Varmint");
        add_item(
            app_menu,
            new_menu_item("About Varmint", Some(sel!(varmintShowAbout:)), "", 0, app),
        );
        add_separator(app_menu);
        add_item(
            app_menu,
            new_menu_item("Quit Varmint", Some(sel!(terminate:)), "q", COMMAND, app),
        );
        add_submenu(main_menu, "Varmint", app_menu);

        let view_menu = new_menu("View");
        let _: () = msg_send![view_menu, setAutoenablesItems: false];
        let statistics_item = new_menu_item(
            "Show Statistics",
            Some(sel!(varmintToggleStatistics:)),
            "s",
            COMMAND | SHIFT,
            app,
        );
        let _: () = msg_send![statistics_item, setEnabled: true];
        add_item(view_menu, statistics_item);
        STATISTICS_ITEM.store(statistics_item, Ordering::Release);
        add_separator(view_menu);
        let fullscreen_item = new_menu_item(
            "Enter Full Screen",
            Some(sel!(toggleFullScreen:)),
            "f",
            COMMAND | CONTROL,
            ptr::null_mut(),
        );
        let _: () = msg_send![fullscreen_item, setEnabled: true];
        add_item(view_menu, fullscreen_item);
        add_submenu(main_menu, "View", view_menu);

        let _: () = msg_send![app, setMainMenu: main_menu];
    }
}

pub fn take_toggle_statistics() -> bool {
    TOGGLE_STATISTICS.swap(false, Ordering::AcqRel)
}

pub fn take_show_about() -> bool {
    SHOW_ABOUT.swap(false, Ordering::AcqRel)
}

pub fn set_statistics_checked(checked: bool) {
    let item = STATISTICS_ITEM.load(Ordering::Acquire);
    if item.is_null() {
        return;
    }

    unsafe {
        let state: isize = if checked { 1 } else { 0 };
        let _: () = msg_send![item, setState: state];
    }
}

pub fn show_about() {
    const DESCRIPTION: &str = "A lightweight gaming VM";
    const REPOSITORY_URL: &str = "https://github.com/kisasexypantera94/varmint";

    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let options: *mut AnyObject = msg_send![class!(NSMutableDictionary), new];

        let name_key = ns_string("ApplicationName");
        let name = ns_string("Varmint");
        let version_key = ns_string("Version");
        let version = ns_string(env!("CARGO_PKG_VERSION"));
        let credits_key = ns_string("Credits");

        let credits_text = format!("{DESCRIPTION}\n\n{REPOSITORY_URL}");
        let credits_string = ns_string(&credits_text);
        let credits: *mut AnyObject = msg_send![class!(NSMutableAttributedString), alloc];
        let credits: *mut AnyObject = msg_send![credits, initWithString: credits_string];

        let link_key = ns_string("NSLink");
        let link_value = ns_string(REPOSITORY_URL);
        let link_range = NSRange {
            location: DESCRIPTION.encode_utf16().count() + 2,
            length: REPOSITORY_URL.encode_utf16().count(),
        };
        let _: () = msg_send![
            credits,
            addAttribute: link_key,
            value: link_value,
            range: link_range
        ];

        let _: () = msg_send![options, setObject: name, forKey: name_key];
        let _: () = msg_send![options, setObject: version, forKey: version_key];
        let _: () = msg_send![options, setObject: credits, forKey: credits_key];
        let _: () = msg_send![app, orderFrontStandardAboutPanelWithOptions: options];

        objc2::ffi::objc_release(name_key);
        objc2::ffi::objc_release(name);
        objc2::ffi::objc_release(version_key);
        objc2::ffi::objc_release(version);
        objc2::ffi::objc_release(credits_key);
        objc2::ffi::objc_release(credits_string);
        objc2::ffi::objc_release(credits);
        objc2::ffi::objc_release(link_key);
        objc2::ffi::objc_release(link_value);
        objc2::ffi::objc_release(options);
    }
}
