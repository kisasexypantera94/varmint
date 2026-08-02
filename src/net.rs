mod helper;

use std::{
    io,
    os::fd::AsRawFd,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};
use uuid::Uuid;

struct Shared {
    interface_id: String,
    connection: Mutex<Option<helper::Connection>>,
    recovering: AtomicBool,
    generation: AtomicU64,
}

pub struct Backend {
    shared: Arc<Shared>,
    mac: [u8; 6],
    max_packet_size: u64,
}

pub fn start(on_frame: impl Fn(Vec<u8>) + Send + 'static) -> io::Result<Backend> {
    let interface_id = Uuid::new_v4().to_string();
    let connection = helper::connect(&interface_id)?;
    let mac = connection.mac;
    let max_packet_size = connection.max_packet_size;
    let shared = Arc::new(Shared {
        interface_id,
        connection: Mutex::new(Some(connection)),
        recovering: AtomicBool::new(false),
        generation: AtomicU64::new(0),
    });

    let rx_shared = shared.clone();
    thread::Builder::new().name("varmint-vmnet-rx".into()).spawn(move || {
        let mut buffer = vec![0; max_packet_size as usize];
        loop {
            let generation = rx_shared.generation.load(Ordering::Acquire);
            let socket = {
                let guard = rx_shared.connection.lock().unwrap();
                let Some(connection) = guard.as_ref() else {
                    drop(guard);
                    thread::sleep(Duration::from_millis(100));
                    continue;
                };
                match connection.socket.try_clone() {
                    Ok(socket) => socket,
                    Err(error) => {
                        drop(guard);
                        if request_recovery(rx_shared.clone(), generation) {
                            eprintln!("vmnet rx socket clone failed: {error}; restarting helper");
                        }
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                }
            };
            let _ = socket.set_read_timeout(Some(Duration::from_secs(1)));

            loop {
                if rx_shared.generation.load(Ordering::Acquire) != generation {
                    break;
                }
                match socket.recv(&mut buffer) {
                    Ok(size) if size >= 14 => on_frame(buffer[..size].to_vec()),
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
                    Err(error) => {
                        if request_recovery(rx_shared.clone(), generation) {
                            eprintln!("vmnet rx error: {error}; restarting helper");
                        }
                        break;
                    }
                }
            }
        }
    })?;

    Ok(Backend {
        shared,
        mac,
        max_packet_size,
    })
}

fn request_recovery(shared: Arc<Shared>, failed_generation: u64) -> bool {
    if shared
        .recovering
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }

    let recovery = shared.clone();
    if let Err(error) = thread::Builder::new()
        .name("varmint-vmnet-recovery".into())
        .spawn(move || {
            if recovery.generation.load(Ordering::Acquire) != failed_generation {
                recovery.recovering.store(false, Ordering::Release);
                return;
            }

            let old = recovery.connection.lock().unwrap().take();
            drop(old);

            loop {
                match helper::connect(&recovery.interface_id) {
                    Ok(connection) => {
                        *recovery.connection.lock().unwrap() = Some(connection);
                        recovery.generation.fetch_add(1, Ordering::Release);
                        eprintln!("vmnet helper restarted");
                        break;
                    }
                    Err(error) => {
                        eprintln!("vmnet helper restart failed: {error}; retrying");
                        thread::sleep(Duration::from_secs(1));
                    }
                }
            }
            recovery.recovering.store(false, Ordering::Release);
        })
    {
        shared.recovering.store(false, Ordering::Release);
        eprintln!("failed to spawn vmnet recovery thread: {error}");
        return false;
    }

    true
}

impl Backend {
    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    pub fn write(&self, frame: &[u8]) {
        // Oversized unix datagrams are silently truncated on the receiving
        // side, so this check is load-bearing.
        if frame.len() > self.max_packet_size as usize {
            eprintln!("vmnet tx error, dropping frame: frame exceeds max packet size");
            return;
        }

        let generation = self.shared.generation.load(Ordering::Acquire);
        let guard = self.shared.connection.lock().unwrap();
        let Some(connection) = guard.as_ref() else {
            return;
        };
        let result = unsafe {
            libc::send(
                connection.socket.as_raw_fd(),
                frame.as_ptr().cast(),
                frame.len(),
                libc::MSG_DONTWAIT,
            )
        };
        drop(guard);

        if result == -1 {
            let error = io::Error::last_os_error();
            if request_recovery(self.shared.clone(), generation) {
                eprintln!("vmnet tx error, dropping frame: {error}; restarting helper");
            }
        }
    }
}
