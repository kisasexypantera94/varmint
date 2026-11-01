//! Output of a VM started from the launcher: the VMM's stdout and stderr go to
//! `~/Library/Logs/Varmint/<VM>.log`, and the last lines are kept for error messages.

use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::{Arc, Mutex},
    thread,
};

/// A VMM stuck in a logging loop must not fill the disk; past this size output is dropped.
const LOG_LIMIT: u64 = 50 * 1024 * 1024;
const TAIL_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub struct VmLog {
    path: PathBuf,
    tail: Arc<Mutex<VecDeque<u8>>>,
}

impl VmLog {
    /// Starts `executable config`, writing its output to the log of the VM named `name`. The
    /// log is replaced on every start.
    pub fn spawn(executable: &Path, config: &Path, name: &str) -> io::Result<(Child, Self)> {
        let directory = std::env::home_dir()
            .ok_or_else(|| io::Error::other("the home directory is unknown"))?
            .join("Library/Logs/Varmint");
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{name}.log"));
        let file = File::create(&path)?;

        let (reader, writer) = io::pipe()?;
        let child = Command::new(executable)
            .arg(config)
            .stdout(writer.try_clone()?)
            .stderr(writer)
            .spawn()?;
        let tail = Arc::new(Mutex::new(VecDeque::with_capacity(TAIL_BYTES)));
        let thread_tail = tail.clone();
        // The pipe is drained until the VMM and its helpers exit, even past the size limit, so
        // they never block on a full pipe.
        thread::spawn(move || copy_output(reader, file, &thread_tail));
        Ok((child, Self { path, tail }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The last non-empty lines the VM printed.
    pub fn last_lines(&self, count: usize) -> String {
        let tail = self.tail.lock().unwrap();
        let (front, back) = tail.as_slices();
        let text = String::from_utf8_lossy(&[front, back].concat()).into_owned();
        let lines = text
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with("note: run with `RUST_BACKTRACE"))
            .collect::<Vec<_>>();
        lines[lines.len().saturating_sub(count)..].join("\n")
    }
}

fn copy_output(mut reader: impl Read, mut file: impl Write, tail: &Mutex<VecDeque<u8>>) {
    let mut buffer = [0; 8192];
    let mut written = 0;
    loop {
        let length = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(length) => length,
        };
        let chunk = &buffer[..length];
        if written < LOG_LIMIT {
            let kept = &chunk[..chunk.len().min((LOG_LIMIT - written) as usize)];
            let _ = file.write_all(kept);
            written += kept.len() as u64;
            if written == LOG_LIMIT {
                let _ = writeln!(file, "\n[Varmint stopped writing this log at {} MB]", LOG_LIMIT >> 20);
            }
        }
        let mut tail = tail.lock().unwrap();
        tail.extend(chunk);
        let excess = tail.len().saturating_sub(TAIL_BYTES);
        tail.drain(..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_the_file_but_keeps_the_latest_output() {
        let output = vec![b'x'; LOG_LIMIT as usize + 100]
            .into_iter()
            .chain(*b"\npanicked: boom\n")
            .collect::<Vec<_>>();
        let mut file = Vec::new();
        let tail = Mutex::new(VecDeque::new());

        copy_output(output.as_slice(), &mut file, &tail);

        assert!(file.len() < LOG_LIMIT as usize + 100);
        assert!(String::from_utf8_lossy(&file).ends_with("[Varmint stopped writing this log at 50 MB]\n"));
        let log = VmLog {
            path: PathBuf::new(),
            tail: Arc::new(tail),
        };
        assert_eq!(log.last_lines(1), "panicked: boom");
    }
}
