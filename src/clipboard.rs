use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread,
    time::Duration,
};

pub struct Sink {
    tx: Sender<Vec<u8>>,
}

impl Sink {
    pub fn push(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(bytes);
    }
}

pub fn start(on_host_change: impl FnMut(Vec<u8>) + Send + 'static) -> Sink {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::Builder::new()
        .name("varmint-clipboard".into())
        .spawn(move || run(rx, on_host_change))
        .unwrap_or_else(|error| panic!("failed to spawn clipboard thread: {error}"));
    Sink { tx }
}

const POLL_INTERVAL: Duration = Duration::from_millis(250);

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

struct Pasteboard {
    inner: objc2::rc::Retained<NSPasteboard>,
}

impl Pasteboard {
    fn general() -> Pasteboard {
        let inner = NSPasteboard::generalPasteboard();
        Pasteboard { inner }
    }

    fn change_count(&self) -> isize {
        self.inner.changeCount()
    }

    fn read_text(&self) -> Option<Vec<u8>> {
        let s = unsafe { self.inner.stringForType(NSPasteboardTypeString) }?;
        Some(s.to_string().into_bytes())
    }

    fn write_text(&self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let ns = NSString::from_str(&text);
        self.inner.clearContents();
        unsafe {
            self.inner.setString_forType(&ns, NSPasteboardTypeString);
        }
    }
}

struct Deframer {
    buf: Vec<u8>,
}

impl Deframer {
    fn new() -> Deframer {
        Deframer { buf: Vec::new() }
    }

    fn push(&mut self, bytes: &[u8], mut on_frame: impl FnMut(Vec<u8>)) {
        self.buf.extend_from_slice(bytes);

        loop {
            if self.buf.len() < FRAME_LEN_PREFIX {
                return;
            }
            let len = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;

            if len > MAX_FRAME {
                self.buf.remove(0);
                continue;
            }

            let total = FRAME_LEN_PREFIX + len;
            if self.buf.len() < total {
                return;
            }

            let payload = self.buf[FRAME_LEN_PREFIX..total].to_vec();
            self.buf.drain(..total);
            on_frame(payload);
        }
    }
}

const FRAME_LEN_PREFIX: usize = 4;

const MAX_FRAME: usize = 16 * 1024 * 1024;

fn run(out_rx: Receiver<Vec<u8>>, mut on_input: impl FnMut(Vec<u8>)) {
    let pb = Pasteboard::general();

    let mut last_change = pb.change_count();
    let mut last_hash = pb.read_text().map(|b| hash_bytes(&b));
    let mut deframer = Deframer::new();

    loop {
        loop {
            match out_rx.try_recv() {
                Ok(bytes) => {
                    let mut to_write: Option<Vec<u8>> = None;
                    deframer.push(&bytes, |frame| to_write = Some(frame));
                    if let Some(frame) = to_write {
                        let h = hash_bytes(&frame);
                        if Some(h) != last_hash {
                            pb.write_text(&frame);
                            last_hash = Some(h);
                            last_change = pb.change_count();
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        let change = pb.change_count();
        if change != last_change {
            last_change = change;
            if let Some(bytes) = pb.read_text() {
                let h = hash_bytes(&bytes);
                if Some(h) != last_hash {
                    last_hash = Some(h);
                    on_input(bytes);
                }
            }
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}
