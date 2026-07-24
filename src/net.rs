mod helper;

pub use helper::Process as Helper;
use std::{io, os::unix::net::UnixDatagram, thread};

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
        let mut last_error = None;
        loop {
            match socket.recv(&mut buffer) {
                Ok(size) => {
                    last_error = None;
                    if size >= 14 {
                        on_frame(buffer[..size].to_vec());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    if last_error != Some(error.kind()) {
                        eprintln!("vmnet rx error: {error}");
                        last_error = Some(error.kind());
                    }
                }
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

    pub fn write(&self, frame: &[u8]) -> io::Result<()> {
        // Oversized unix datagrams are silently truncated on the receiving
        // side, so this check is load-bearing.
        if frame.len() > self.max_packet_size as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Ethernet frame exceeds vmnet maximum packet size",
            ));
        }
        self.socket.send(frame).map(|_| ())
    }
}
