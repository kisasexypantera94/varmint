mod helper;

pub use helper::Process as Helper;
use std::{
    io,
    os::{fd::AsRawFd, unix::net::UnixDatagram},
    thread,
};

pub struct Backend {
    socket: UnixDatagram,
    mac: [u8; 6],
    max_packet_size: u64,
}

pub fn start(on_frame: impl Fn(Vec<u8>) + Send + 'static) -> io::Result<(Backend, Helper)> {
    let connection = helper::connect()?;
    let socket = connection.socket.try_clone()?;
    let buffer_size = connection.max_packet_size as usize;

    thread::Builder::new().name("varmint-vmnet-rx".into()).spawn(move || {
        let mut buffer = vec![0; buffer_size];
        loop {
            match socket.recv(&mut buffer) {
                Ok(size) if size >= 14 => on_frame(buffer[..size].to_vec()),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => eprintln!("vmnet rx error: {error}"),
            }
        }
    })?;

    Ok((
        Backend {
            socket: connection.socket,
            mac: connection.mac,
            max_packet_size: connection.max_packet_size,
        },
        connection.process,
    ))
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

        let result = unsafe {
            libc::send(
                self.socket.as_raw_fd(),
                frame.as_ptr().cast(),
                frame.len(),
                libc::MSG_DONTWAIT,
            )
        };

        if result == -1 {
            eprintln!("vmnet tx error, dropping frame: {}", io::Error::last_os_error());
        }
    }
}
