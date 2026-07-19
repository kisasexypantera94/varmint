use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
};

const KERNEL_ENV: &str = "VARMINT_KERNEL";
const INITRD_ENV: &str = "VARMINT_INITRD";
const DTB_ENV: &str = "VARMINT_DTB";
const DISK_ENV: &str = "VARMINT_DISK";

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub kernel: PathBuf,
    pub initrd: PathBuf,
    pub dtb: PathBuf,
    pub disk: PathBuf,
}

impl RuntimePaths {
    pub fn resolve() -> Self {
        let resources = bundle_resources();

        Self {
            kernel: env_path(KERNEL_ENV)
                .unwrap_or_else(|| resource_path(resources.as_deref(), "Image", "artifacts/kernel/Image")),
            initrd: env_path(INITRD_ENV)
                .unwrap_or_else(|| resource_path(resources.as_deref(), "initrd", "artifacts/kernel/initrd")),
            dtb: env_path(DTB_ENV)
                .unwrap_or_else(|| resource_path(resources.as_deref(), "varmint.dtb", "artifacts/guest.dtb")),
            disk: env_path(DISK_ENV)
                .or_else(|| argument_path("--disk"))
                .unwrap_or_else(|| default_disk_path(resources.is_some())),
        }
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn argument_path(flag: &str) -> Option<PathBuf> {
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        if argument.as_os_str() == OsStr::new(flag) {
            let value = args.next().unwrap_or_else(|| panic!("{flag} requires a path"));
            return Some(PathBuf::from(value));
        }
    }
    None
}

fn default_disk_path(in_bundle: bool) -> PathBuf {
    if in_bundle {
        panic!("no VM disk selected; launch with --disk /path/to/disk.raw or set VARMINT_DISK");
    }
    PathBuf::from("dev0.img")
}

fn bundle_resources() -> Option<PathBuf> {
    let executable = env::current_exe().ok()?;
    let macos = executable.parent()?;
    if macos.file_name()? != OsStr::new("MacOS") {
        return None;
    }

    Some(macos.parent()?.join("Resources/kernel"))
}

fn resource_path(resources: Option<&Path>, bundled_name: &str, development_path: &str) -> PathBuf {
    resources
        .map(|resources| resources.join(bundled_name))
        .unwrap_or_else(|| PathBuf::from(development_path))
}
