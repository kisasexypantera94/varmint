use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::{
    io::{self, IsTerminal, Read, Write},
    thread,
};

pub struct Sink;

impl Sink {
    pub fn write(&self, byte: u8) {
        let mut stdout = io::stdout();
        let _ = stdout.write_all(&[byte]);
        let _ = stdout.flush();
    }
}

pub fn start(on_byte: impl Fn(u8) + Send + 'static) -> Sink {
    if io::stdin().is_terminal() {
        thread::Builder::new()
            .name("varmint-serial-rx".into())
            .spawn(move || read_loop(on_byte))
            .unwrap_or_else(|error| panic!("failed to spawn serial input thread: {error}"));
    }
    Sink
}

fn read_loop(on_byte: impl Fn(u8)) {
    let _raw = RawModeGuard::new().unwrap();
    let stdin = io::stdin();
    let mut buf = [0u8; 1];

    const PREFIX: u8 = 0x1d;
    let mut got_prefix = false;

    eprintln!("[VM] Press Ctrl-] x to detach the serial console");

    loop {
        match stdin.lock().read(&mut buf) {
            Ok(0) => break,
            Ok(_) => {
                let b = buf[0];

                if got_prefix {
                    got_prefix = false;
                    match b {
                        b'x' => {
                            eprintln!("Received break command");
                            break;
                        }
                        _ => eprint!("unknown command: {b:#x}\r\n"),
                    }
                    continue;
                }

                if b == PREFIX {
                    got_prefix = true;
                    continue;
                }

                on_byte(b);
            }
            Err(e) => {
                eprintln!("stdin read error: {e}");
                break;
            }
        }
    }
}

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}
