use serde::Deserialize;
use std::{
    env,
    fs::{self, DirBuilder, File, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::{fs::DirBuilderExt, net::UnixDatagram},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const WATCHDOG_SCRIPT: &str =
    r#"while kill -0 "$1" 2>/dev/null && kill -0 "$2" 2>/dev/null; do sleep 2; done; kill "$2" 2>/dev/null"#;

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
    child: Child,
    watchdog: Child,
    directory: PathBuf,
}

pub fn connect(interface_id: &str) -> io::Result<Connection> {
    let helper = helper_path()?;
    let log = log_path()?;
    let directory = temporary_directory()?;
    let helper_socket = directory.join("helper.sock");
    let client_socket = directory.join("client.sock");
    let info_path = directory.join("interface.json");

    let mut process = match launch(
        &helper,
        &helper_socket,
        &info_path,
        &log,
        interface_id,
        directory.clone(),
    ) {
        Ok(process) => process,
        Err(error) => {
            let _ = fs::remove_dir_all(&directory);
            return Err(error);
        }
    };

    let info = wait_for_info(&mut process, &info_path, &helper_socket)?;
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
        let _ = self.watchdog.kill();
        let _ = self.watchdog.wait();

        let pid = self.child.id() as libc::pid_t;
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
                Ok(None) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn launch(
    helper: &Path,
    socket: &Path,
    info: &Path,
    log: &Path,
    interface_id: &str,
    directory: PathBuf,
) -> io::Result<Process> {
    let stdout = File::create(info).map_err(|error| path_error("create", info, error))?;
    let stderr = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|error| path_error("open", log, error))?;
    let mut child = Command::new(helper)
        .arg("--socket")
        .arg(socket)
        .arg("--interface-id")
        .arg(interface_id)
        .arg("--operation-mode")
        .arg("shared")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .map_err(|error| io::Error::new(error.kind(), format!("failed to start vmnet-helper: {error}")))?;

    let watchdog = match launch_watchdog(child.id()) {
        Ok(watchdog) => watchdog,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };

    Ok(Process {
        child,
        watchdog,
        directory,
    })
}

fn launch_watchdog(helper_pid: u32) -> io::Result<Child> {
    Command::new("/bin/sh")
        .args(["-c", WATCHDOG_SCRIPT, "varmint-vmnet-watchdog"])
        .arg(std::process::id().to_string())
        .arg(helper_pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| io::Error::new(error.kind(), format!("failed to start vmnet watchdog: {error}")))
}

fn wait_for_info(process: &mut Process, path: &Path, socket: &Path) -> io::Result<Info> {
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
        if process.child.try_wait()?.is_some() {
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
    Ok(directory.join("Varmint.log"))
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
