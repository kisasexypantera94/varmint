use serde::Deserialize;
use std::{
    env, fs,
    fs::DirBuilder,
    io,
    os::{
        fd::AsRawFd,
        unix::{fs::DirBuilderExt, net::UnixDatagram},
    },
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const SCRIPT: &str = "on run argv\ndo shell script (item 1 of argv) with administrator privileges\nend run";

#[derive(Deserialize)]
struct Info {
    vmnet_mac_address: String,
    vmnet_max_packet_size: u64,
}

pub struct Connection {
    pub socket: UnixDatagram,
    pub mac: [u8; 6],
    pub max_packet_size: u64,
    pub process: Process,
}

pub struct Process {
    pid: libc::pid_t,
    directory: PathBuf,
}

pub fn connect() -> io::Result<Connection> {
    let helper = helper_path()?;
    let log = log_path()?;
    let directory = temporary_directory()?;
    let helper_socket = directory.join("helper.sock");
    let client_socket = directory.join("client.sock");
    let info_path = directory.join("interface.json");

    let pid = match launch(&helper, &helper_socket, &info_path, &log) {
        Ok(pid) => pid,
        Err(error) => {
            let _ = fs::remove_dir_all(&directory);
            return Err(error);
        }
    };
    let process = Process { pid, directory };

    let info = wait_for_info(pid, &info_path, &helper_socket)?;
    let mac = parse_mac(&info.vmnet_mac_address)?;
    if !(14..=64 * 1024).contains(&info.vmnet_max_packet_size) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid vmnet maximum packet size: {}", info.vmnet_max_packet_size),
        ));
    }

    let socket = UnixDatagram::bind(&client_socket).map_err(|error| path_error("bind", &client_socket, error))?;
    set_buffer_sizes(&socket, 2 * 1024 * 1024)?;
    socket
        .connect(&helper_socket)
        .map_err(|error| path_error("connect to", &helper_socket, error))?;

    Ok(Connection {
        socket,
        mac,
        max_packet_size: info.vmnet_max_packet_size,
        process,
    })
}

impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.pid, libc::SIGTERM);
        }
        for _ in 0..40 {
            if !process_exists(self.pid) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn launch(helper: &Path, socket: &Path, info: &Path, log: &Path) -> io::Result<libc::pid_t> {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    // SUDO_UID/SUDO_GID make the helper drop privileges back to us after
    // opening vmnet, which is also what makes SIGTERM from Drop possible.
    // The watchdog covers exit paths that skip Drop (Cmd+Q terminate:,
    // force quit, crashes); Drop remains the fast path on clean shutdown.
    let watched = std::process::id();
    let command = format!(
        "umask 022; export SUDO_UID={uid} SUDO_GID={gid}; {} --socket {} --operation-mode shared </dev/null > {} 2>> {} & VARMINT_HELPER=$!; (while kill -0 {watched} 2>/dev/null && kill -0 $VARMINT_HELPER 2>/dev/null; do sleep 2; done; kill $VARMINT_HELPER 2>/dev/null) >/dev/null 2>&1 & echo $VARMINT_HELPER",
        quote(helper),
        quote(socket),
        quote(info),
        quote(log),
    );
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", SCRIPT, "--"])
        .arg(command)
        .output()
        .map_err(|error| io::Error::new(error.kind(), format!("failed to request vmnet privileges: {error}")))?;

    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        let message = message.trim();
        return Err(io::Error::other(if message.is_empty() {
            format!("osascript exited with {}", output.status)
        } else {
            message.to_owned()
        }));
    }

    let value = String::from_utf8_lossy(&output.stdout);
    let pid = value.trim().parse::<libc::pid_t>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid vmnet-helper PID: {}", value.trim()),
        )
    })?;
    (pid > 0)
        .then_some(pid)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("invalid vmnet-helper PID: {pid}")))
}

fn wait_for_info(pid: libc::pid_t, path: &Path, socket: &Path) -> io::Result<Info> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if socket.exists() {
            let data = fs::read_to_string(path).map_err(|error| path_error("read", path, error))?;
            if !data.trim().is_empty() {
                match serde_json::from_str(&data) {
                    Ok(info) => return Ok(info),
                    Err(error) if error.is_eof() => {}
                    Err(error) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("invalid vmnet-helper response: {error}: {}", data.trim()),
                        ));
                    }
                }
            }
        }
        if !process_exists(pid) {
            return Err(io::Error::other("vmnet-helper exited before startup"));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for vmnet-helper startup",
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn set_buffer_sizes(socket: &UnixDatagram, size: libc::c_int) -> io::Result<()> {
    // Default unix datagram buffers on macOS hold only a handful of Ethernet
    // frames; under load that means silent drops and send errors.
    for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
        let result = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                option,
                (&raw const size).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if result != 0 {
            let error = io::Error::last_os_error();
            return Err(io::Error::new(
                error.kind(),
                format!("failed to size vmnet socket buffers: {error}"),
            ));
        }
    }
    Ok(())
}

fn process_exists(pid: libc::pid_t) -> bool {
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    alive || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn helper_path() -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let contents = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("cannot locate Varmint.app Contents directory"))?;
    let helper = contents.join("Helpers/vmnet-helper");
    if helper.is_file() {
        Ok(helper)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("bundled vmnet-helper is missing: {}", helper.display()),
        ))
    }
}

fn log_path() -> io::Result<PathBuf> {
    let home = env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?;
    let directory = PathBuf::from(home).join("Library/Logs");
    fs::create_dir_all(&directory).map_err(|error| path_error("create", &directory, error))?;
    let path = directory.join("Varmint.log");
    // Pre-create as the current user; the helper appends to it as root and
    // must not end up owning the file.
    if !path.exists() {
        fs::File::create(&path).map_err(|error| path_error("create", &path, error))?;
    }
    Ok(path)
}

fn temporary_directory() -> io::Result<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let directory = Path::new("/tmp").join(format!("varmint-vmnet-{}-{nonce}", std::process::id()));
    DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|error| path_error("create", &directory, error))?;
    Ok(directory)
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn path_error(action: &str, path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("failed to {action} {}: {error}", path.display()))
}

fn parse_mac(value: &str) -> io::Result<[u8; 6]> {
    let mut parts = value.split(':');
    let mut mac = [0; 6];
    for byte in &mut mac {
        let part = parts.next().ok_or_else(|| invalid_mac(value))?;
        *byte = u8::from_str_radix(part, 16).map_err(|_| invalid_mac(value))?;
    }
    if parts.next().is_some() {
        return Err(invalid_mac(value));
    }
    Ok(mac)
}

fn invalid_mac(value: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid vmnet MAC address: {value}"),
    )
}
