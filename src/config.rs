use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const DEFAULT_DATA_DISK_CAPACITY_GIB: u64 = 128;
const DEFAULT_KERNEL_ARGS: &str = "console=ttyAMA0 earlycon=pl011,mmio32,0x09000000 root=LABEL=varmint-root rw";
const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

pub fn locate() -> Option<PathBuf> {
    let mut arguments = env::args_os().skip(1);
    let config = match arguments.next() {
        Some(path) => PathBuf::from(path),
        None => return choose_config(),
    };
    if let Some(argument) = arguments.next() {
        panic!("unexpected argument: {}", argument.to_string_lossy());
    }
    Some(config)
}

fn choose_config() -> Option<PathBuf> {
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg("POSIX path of (choose file with prompt \"Choose a Varmint configuration\" of type {\"varmint\"})")
        .output()
        .unwrap_or_else(|error| panic!("failed to open configuration picker: {error}"));

    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("configuration path is not valid UTF-8: {error}"));
    let path = path.trim_end_matches(['\r', '\n']);

    (!path.is_empty()).then_some(PathBuf::from(path))
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NeptuneBackend {
    #[default]
    Dxmt,
    D3dmetal,
}

impl NeptuneBackend {
    pub fn as_env(self) -> &'static str {
        match self {
            Self::Dxmt => "dxmt",
            Self::D3dmetal => "d3dmetal",
        }
    }

    pub fn from_env(value: &str) -> Option<Self> {
        match value {
            "dxmt" => Some(Self::Dxmt),
            "d3dmetal" => Some(Self::D3dmetal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VmConfig {
    pub memory_size: usize,
    pub vcpus: usize,
    pub disk: PathBuf,
    pub data_disk: PathBuf,
    pub data_disk_capacity: u64,
    pub base_image: PathBuf,
    pub kernel: PathBuf,
    pub initrd: Option<PathBuf>,
    pub kernel_args: String,
    pub neptune_backend: NeptuneBackend,
    pub host_environment: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VmConfigFile {
    format_version: u32,
    #[serde(default = "default_memory_mib")]
    memory_mib: u64,
    #[serde(default = "default_vcpus")]
    vcpus: usize,
    storage_dir: Option<PathBuf>,
    #[serde(default = "default_data_disk_capacity_gib")]
    data_disk_capacity_gib: u64,
    base_image: Option<PathBuf>,
    kernel: Option<PathBuf>,
    initrd: Option<PathBuf>,
    kernel_args: Option<Vec<String>>,
    #[serde(default)]
    neptune_backend: NeptuneBackend,
    #[serde(default)]
    host_environment: BTreeMap<String, String>,
}

impl VmConfig {
    pub fn load(path: &Path) -> Self {
        let path = absolute_path(path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read VM config {}: {error}", path.display()));
        let config: VmConfigFile = toml::from_str(&source)
            .unwrap_or_else(|error| panic!("failed to parse VM config {}: {error}", path.display()));

        if config.format_version != 2 {
            panic!(
                "unsupported VM config format_version {}; expected 2",
                config.format_version
            );
        }
        if config.vcpus == 0 {
            panic!("vcpus must be greater than zero");
        }
        if config.memory_mib < 256 {
            panic!("memory_mib must be at least 256");
        }
        if config.data_disk_capacity_gib == 0 {
            panic!("data_disk_capacity_gib must be greater than zero");
        }
        if config.kernel.is_none() && config.initrd.is_some() {
            panic!("initrd requires a custom kernel");
        }

        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let resources = bundle_resources();
        let storage_dir = config
            .storage_dir
            .map(|path| resolve_path(base, path))
            .unwrap_or_else(|| {
                let name = path
                    .file_stem()
                    .unwrap_or_else(|| panic!("VM config path has no file name: {}", path.display()));
                base.join(name)
            });
        let disk = storage_dir.join("system.raw");
        let data_disk = storage_dir.join("data.raw");
        let base_image = config
            .base_image
            .map(|path| resolve_path(base, path))
            .unwrap_or_else(|| {
                resource_path(
                    resources.as_deref(),
                    "runtime/base.raw.zst",
                    "build/guest/varmint-debian.raw.zst",
                )
            });
        let memory_size = bytes_from_units(config.memory_mib, MIB, "memory_mib");
        let data_disk_capacity = config
            .data_disk_capacity_gib
            .checked_mul(GIB)
            .unwrap_or_else(|| panic!("data_disk_capacity_gib is too large"));

        let (kernel, initrd) = match config.kernel {
            Some(kernel) => (
                resolve_path(base, kernel),
                config.initrd.map(|initrd| resolve_path(base, initrd)),
            ),
            None => {
                let kernel = resource_path(resources.as_deref(), "kernel/Image", "build/guest/Image");
                let initrd = resource_path(resources.as_deref(), "kernel/initrd", "build/guest/initrd");
                (kernel, Some(initrd))
            }
        };

        let kernel_args = config
            .kernel_args
            .map(|args| args.join(" "))
            .unwrap_or_else(|| DEFAULT_KERNEL_ARGS.to_owned());

        Self {
            memory_size,
            vcpus: config.vcpus,
            disk,
            data_disk,
            data_disk_capacity,
            base_image,
            kernel,
            initrd,
            kernel_args,
            neptune_backend: config.neptune_backend,
            host_environment: config.host_environment,
        }
    }
}

fn default_memory_mib() -> u64 {
    let host_bytes = Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(32 * GIB);

    (host_bytes / 2 / MIB).max(256)
}

fn default_vcpus() -> usize {
    let host = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(8);
    let target = (host * 3 / 4).max(1);
    target.min(host.saturating_sub(2).max(1))
}

fn default_data_disk_capacity_gib() -> u64 {
    DEFAULT_DATA_DISK_CAPACITY_GIB
}

fn bytes_from_units(value: u64, unit: u64, name: &str) -> usize {
    let bytes = value.checked_mul(unit).unwrap_or_else(|| panic!("{name} is too large"));
    usize::try_from(bytes).unwrap_or_else(|_| panic!("{name} is too large for this host"))
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_owned();
    }

    env::current_dir()
        .unwrap_or_else(|error| panic!("failed to resolve current directory: {error}"))
        .join(path)
}

fn resolve_path(base: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() { path } else { base.join(path) }
}

fn bundle_resources() -> Option<PathBuf> {
    let executable = env::current_exe().ok()?;
    let macos = executable.parent()?;
    if macos.file_name()? != OsStr::new("MacOS") {
        return None;
    }

    Some(macos.parent()?.join("Resources"))
}

fn resource_path(resources: Option<&Path>, bundled_name: &str, development_path: &str) -> PathBuf {
    resources
        .map(|resources| resources.join(bundled_name))
        .unwrap_or_else(|| PathBuf::from(development_path))
}
