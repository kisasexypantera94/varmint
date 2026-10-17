use crate::config::VmConfig;
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Write},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
};

const DATA_DISK_MAGIC: &[u8] = b"VARMINT-DATA-V1";

pub struct Guard {
    _lock: File,
}

pub fn prepare(config: &VmConfig) -> io::Result<Guard> {
    let guard = Guard {
        _lock: lock(&config.disk)?,
    };

    prepare_system_disk(config)?;
    prepare_data_disk(&config.data_disk, config.data_disk_size)?;
    ensure_distinct(&config.disk, &config.data_disk)?;

    Ok(guard)
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    value.into()
}

fn temporary_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(".partial.{}", std::process::id()));
    parent.join(name)
}

fn lock(disk: &Path) -> io::Result<File> {
    let path = sidecar_path(disk, ".vm-lock");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = OpenOptions::new().read(true).write(true).create(true).open(&path)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(file);
    }

    let error = io::Error::last_os_error();
    Err(io::Error::new(
        error.kind(),
        format!("VM using {} is already running: {error}", disk.display()),
    ))
}

fn read_image_id(path: &Path) -> io::Result<String> {
    let value = fs::read_to_string(path)?;
    let value = value.trim();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid system image id in {}", path.display()),
        ));
    }
    Ok(value.to_ascii_lowercase())
}

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

fn write_image_id(path: &Path, image_id: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let temporary = temporary_path(path);
    let _ = fs::remove_file(&temporary);

    let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
    writeln!(file, "{image_id}")?;
    file.sync_all()?;
    drop(file);

    fs::rename(&temporary, path)?;
    sync_directory(parent)
}

fn prepare_system_disk(config: &VmConfig) -> io::Result<()> {
    let bundled_id_path = config.base_image.with_file_name("system-image-id");
    let expected_id = read_image_id(&bundled_id_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("failed to read {}: {error}", bundled_id_path.display()),
        )
    })?;

    let installed_id_path = sidecar_path(&config.disk, ".image-id");
    let installed_id = read_image_id(&installed_id_path).ok();

    if config.disk.exists() && installed_id.as_deref() == Some(expected_id.as_str()) {
        return Ok(());
    }

    let parent = config.disk.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let temporary = temporary_path(&config.disk);
    let _ = fs::remove_file(&temporary);

    let result = (|| -> io::Result<()> {
        let source = File::open(&config.base_image)?;
        let output = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        let mut output = BufWriter::new(output);

        zstd::stream::copy_decode(BufReader::new(source), &mut output)?;
        output.flush()?;

        let output = output.into_inner().map_err(|error| error.into_error())?;
        output.sync_all()?;
        drop(output);

        fs::rename(&temporary, &config.disk)?;
        sync_directory(parent)?;
        write_image_id(&installed_id_path, &expected_id)
    })();

    let _ = fs::remove_file(&temporary);
    result?;

    match installed_id {
        Some(old) => eprintln!(
            "updated VM system image {}: {} -> {}",
            config.disk.display(),
            old,
            expected_id
        ),
        None => eprintln!("installed VM system image {}: {}", config.disk.display(), expected_id),
    }

    Ok(())
}

fn prepare_data_disk(path: &Path, size: u64) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let temporary = temporary_path(path);
    let _ = fs::remove_file(&temporary);

    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.set_len(size)?;
        file.write_all(DATA_DISK_MAGIC)?;
        file.sync_all()?;

        match fs::hard_link(&temporary, path) {
            Ok(()) => sync_directory(parent),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    })();

    let _ = fs::remove_file(&temporary);
    result
}

fn ensure_distinct(system: &Path, data: &Path) -> io::Result<()> {
    let system = fs::metadata(system)?;
    let data = fs::metadata(data)?;

    if system.dev() == data.dev() && system.ino() == data.ino() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "system and data disk must be distinct files",
        ));
    }

    Ok(())
}
