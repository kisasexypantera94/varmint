use crate::virtio::input::keys::*;
use applevisor::prelude::*;
use minifb::{Key, KeyRepeat, Window, WindowOptions};

pub fn reg_from_rt(rt: u64) -> Option<Reg> {
    match rt {
        0 => Some(Reg::X0),
        1 => Some(Reg::X1),
        2 => Some(Reg::X2),
        3 => Some(Reg::X3),
        4 => Some(Reg::X4),
        5 => Some(Reg::X5),
        6 => Some(Reg::X6),
        7 => Some(Reg::X7),
        8 => Some(Reg::X8),
        9 => Some(Reg::X9),
        10 => Some(Reg::X10),
        11 => Some(Reg::X11),
        12 => Some(Reg::X12),
        13 => Some(Reg::X13),
        14 => Some(Reg::X14),
        15 => Some(Reg::X15),
        16 => Some(Reg::X16),
        17 => Some(Reg::X17),
        18 => Some(Reg::X18),
        19 => Some(Reg::X19),
        20 => Some(Reg::X20),
        21 => Some(Reg::X21),
        22 => Some(Reg::X22),
        23 => Some(Reg::X23),
        24 => Some(Reg::X24),
        25 => Some(Reg::X25),
        26 => Some(Reg::X26),
        27 => Some(Reg::X27),
        28 => Some(Reg::X28),
        29 => Some(Reg::X29),
        30 => Some(Reg::X30),
        31 => None, // XZR/WZR: zero register, not a real stored register
        _ => None,
    }
}

pub fn set_rt(vcpu: &Vcpu, rt: u64, value: u64) -> Result<()> {
    if let Some(reg) = reg_from_rt(rt) {
        vcpu.set_reg(reg, value)?;
    }

    Ok(())
}

pub fn get_rt(vcpu: &Vcpu, rt: u64) -> Result<u64> {
    Ok(match reg_from_rt(rt) {
        Some(reg) => vcpu.get_reg(reg)?,
        None => 0, // XZR/WZR
    })
}

pub fn minifb_to_linux_key(key: Key) -> Option<u16> {
    Some(match key {
        Key::Escape => KEY_ESC,

        Key::Key1 => KEY_1,
        Key::Key2 => KEY_2,
        Key::Key3 => KEY_3,
        Key::Key4 => KEY_4,
        Key::Key5 => KEY_5,
        Key::Key6 => KEY_6,
        Key::Key7 => KEY_7,
        Key::Key8 => KEY_8,
        Key::Key9 => KEY_9,
        Key::Key0 => KEY_0,

        Key::Minus => KEY_MINUS,
        Key::Equal => KEY_EQUAL,
        Key::Backspace => KEY_BACKSPACE,
        Key::Tab => KEY_TAB,

        Key::Q => KEY_Q,
        Key::W => KEY_W,
        Key::E => KEY_E,
        Key::R => KEY_R,
        Key::T => KEY_T,
        Key::Y => KEY_Y,
        Key::U => KEY_U,
        Key::I => KEY_I,
        Key::O => KEY_O,
        Key::P => KEY_P,
        Key::LeftBracket => KEY_LEFTBRACE,
        Key::RightBracket => KEY_RIGHTBRACE,

        Key::Enter => KEY_ENTER,
        Key::LeftCtrl => KEY_LEFTCTRL,

        Key::A => KEY_A,
        Key::S => KEY_S,
        Key::D => KEY_D,
        Key::F => KEY_F,
        Key::G => KEY_G,
        Key::H => KEY_H,
        Key::J => KEY_J,
        Key::K => KEY_K,
        Key::L => KEY_L,
        Key::Semicolon => KEY_SEMICOLON,
        Key::Apostrophe => KEY_APOSTROPHE,
        Key::Backquote => KEY_GRAVE,

        Key::LeftShift => KEY_LEFTSHIFT,
        Key::Backslash => KEY_BACKSLASH,

        Key::Z => KEY_Z,
        Key::X => KEY_X,
        Key::C => KEY_C,
        Key::V => KEY_V,
        Key::B => KEY_B,
        Key::N => KEY_N,
        Key::M => KEY_M,
        Key::Comma => KEY_COMMA,
        Key::Period => KEY_DOT,
        Key::Slash => KEY_SLASH,
        Key::RightShift => KEY_RIGHTSHIFT,

        Key::LeftAlt => KEY_LEFTALT,
        Key::Space => KEY_SPACE,
        Key::CapsLock => KEY_CAPSLOCK,

        Key::F1 => KEY_F1,
        Key::F2 => KEY_F2,
        Key::F3 => KEY_F3,
        Key::F4 => KEY_F4,
        Key::F5 => KEY_F5,
        Key::F6 => KEY_F6,
        Key::F7 => KEY_F7,
        Key::F8 => KEY_F8,
        Key::F9 => KEY_F9,
        Key::F10 => KEY_F10,
        Key::F11 => KEY_F11,
        Key::F12 => KEY_F12,

        Key::NumLock => KEY_NUMLOCK,
        Key::ScrollLock => KEY_SCROLLLOCK,

        Key::NumPad7 => KEY_KP7,
        Key::NumPad8 => KEY_KP8,
        Key::NumPad9 => KEY_KP9,
        Key::NumPadMinus => KEY_KPMINUS,
        Key::NumPad4 => KEY_KP4,
        Key::NumPad5 => KEY_KP5,
        Key::NumPad6 => KEY_KP6,
        Key::NumPadPlus => KEY_KPPLUS,
        Key::NumPad1 => KEY_KP1,
        Key::NumPad2 => KEY_KP2,
        Key::NumPad3 => KEY_KP3,
        Key::NumPad0 => KEY_KP0,
        Key::NumPadDot => KEY_KPDOT,
        Key::NumPadEnter => KEY_KPENTER,
        Key::NumPadSlash => KEY_KPSLASH,
        Key::NumPadAsterisk => KEY_KPASTERISK,

        Key::RightCtrl => KEY_RIGHTCTRL,
        Key::RightAlt => KEY_RIGHTALT,

        Key::Home => KEY_HOME,
        Key::Up => KEY_UP,
        Key::PageUp => KEY_PAGEUP,
        Key::Left => KEY_LEFT,
        Key::Right => KEY_RIGHT,
        Key::End => KEY_END,
        Key::Down => KEY_DOWN,
        Key::PageDown => KEY_PAGEDOWN,
        Key::Insert => KEY_INSERT,
        Key::Delete => KEY_DELETE,

        _ => return None,
    })
}
