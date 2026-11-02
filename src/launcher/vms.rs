//! Where VMs live: their config files and storage on disk, and the list of VMs the launcher knows.

use crate::{
    config::{VmConfigDocument, storage_dir_for},
    storage,
};
use objc2_foundation::{NSArray, NSFileManager, NSString, NSURL, NSUserDefaults};
use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

const KNOWN_VMS_KEY: &str = "KnownVMs";

pub fn vm_name(path: &Path) -> &str {
    path.file_stem().and_then(|name| name.to_str()).unwrap_or("Varmint")
}

pub fn first_known_vm() -> Option<PathBuf> {
    known_vms().into_iter().next()
}

/// Known VMs whose configs still exist, most recently opened first, followed by any VM in the
/// default folder that the list is missing, so VMs Varmint created there can't get lost.
pub fn known_vms() -> Vec<PathBuf> {
    let mut paths = stored_vm_paths()
        .into_iter()
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    if let Ok(directory) = default_vms_directory() {
        for path in vm_configs_in(&directory) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

fn vm_configs_in(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "varmint") && path.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

/// Sorted by name, so sidebar rows don't move when a VM is opened (the stored list is most
/// recent first).
pub fn sidebar_vms() -> Vec<PathBuf> {
    let mut paths = known_vms();
    paths.sort_by_cached_key(|path| vm_name(path).to_lowercase());
    paths
}

pub fn remember_vm(path: &Path) {
    let mut paths = stored_vm_paths();
    paths.retain(|known| known != path);
    paths.insert(0, path.to_owned());
    store_vm_paths(&paths);
}

pub fn forget_vm(path: &Path) {
    let mut paths = stored_vm_paths();
    paths.retain(|known| known != path && known.is_file());
    store_vm_paths(&paths);
}

pub fn replace_known_vm(old_path: &Path, new_path: &Path) {
    let mut paths = stored_vm_paths();
    for path in &mut paths {
        if path == old_path {
            *path = new_path.to_owned();
        }
    }
    store_vm_paths(&paths);
}

fn stored_vm_paths() -> Vec<PathBuf> {
    let Some(paths) = NSUserDefaults::standardUserDefaults().stringArrayForKey(&NSString::from_str(KNOWN_VMS_KEY))
    else {
        return Vec::new();
    };
    (0..paths.count())
        .map(|index| PathBuf::from(paths.objectAtIndex(index).to_string()))
        .collect()
}

fn store_vm_paths(paths: &[PathBuf]) {
    let strings = paths
        .iter()
        .map(|path| NSString::from_str(&path.to_string_lossy()))
        .collect::<Vec<_>>();
    let array = NSArray::from_retained_slice(&strings);
    unsafe {
        NSUserDefaults::standardUserDefaults().setObject_forKey(Some(&array), &NSString::from_str(KNOWN_VMS_KEY))
    };
}

pub fn create_vm_config(name: &str) -> Result<PathBuf, String> {
    create_vm_config_in(name, &default_vms_directory()?)
}

pub fn default_vms_directory() -> Result<PathBuf, String> {
    let home = NSFileManager::defaultManager().homeDirectoryForCurrentUser();
    let home = home
        .path()
        .ok_or_else(|| "The current user's home directory is unavailable.".to_owned())?;
    Ok(PathBuf::from(home.to_string())
        .join("Library")
        .join("Application Support")
        .join("Varmint")
        .join("VMs"))
}

/// "My VM", or "My VM 2", "My VM 3", … when that config or its storage already exists.
pub fn unused_vm_name(directory: &Path) -> String {
    (1..)
        .map(|number| match number {
            1 => "My VM".to_owned(),
            number => format!("My VM {number}"),
        })
        .find(|name| !directory.join(format!("{name}.varmint")).exists() && !directory.join(name).exists())
        .expect("some VM name is unused")
}

fn create_vm_config_in(name: &str, directory: &Path) -> Result<PathBuf, String> {
    let name = validate_vm_name(name)?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    if directory.join(name).exists() {
        return Err(format!("A storage directory named “{name}” already exists."));
    }
    let path = directory.join(format!("{name}.varmint"));
    write_new_config(&path, b"format_version = 2\n", name)?;
    Ok(path)
}

fn write_new_config(path: &Path, contents: &[u8], name: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                format!("A virtual machine named “{name}” already exists.")
            } else {
                error.to_string()
            }
        })?;
    file.write_all(contents).map_err(|error| error.to_string())
}

fn implicit_storage_dir(config_path: &Path) -> PathBuf {
    storage_dir_for(config_path, None)
}

pub fn storage_dir(config_path: &Path, document: &VmConfigDocument) -> PathBuf {
    storage_dir_for(config_path, document.storage_dir())
}

/// Takes the VMM's storage lock so storage isn't moved or copied under a VM started elsewhere,
/// for example from the command line.
pub fn lock_storage(storage: &Path) -> Result<Option<File>, String> {
    if !storage.is_dir() {
        return Ok(None);
    }
    storage::lock(&storage.join("system.raw")).map(Some).map_err(|error| {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            "The virtual machine is running. Quit it and try again.".to_owned()
        } else {
            error.to_string()
        }
    })
}

pub fn rename_vm_files(path: &Path, document: &VmConfigDocument, name: &str) -> Result<PathBuf, String> {
    let name = validate_vm_name(name)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let new_path = parent.join(format!("{name}.varmint"));
    if new_path == path {
        return Ok(new_path);
    }
    if new_path.exists() && !same_file(path, &new_path) {
        return Err(format!("A virtual machine named “{name}” already exists."));
    }

    let storage_move = if document.storage_dir().is_none() {
        let old_storage = implicit_storage_dir(path);
        let new_storage = parent.join(name);
        if new_storage.exists() && !same_file(&old_storage, &new_storage) {
            return Err(format!("A storage directory named “{name}” already exists."));
        }
        old_storage.exists().then_some((old_storage, new_storage))
    } else {
        None
    };

    let _lock = match &storage_move {
        Some((old_storage, _)) => lock_storage(old_storage)?,
        None => None,
    };
    if let Some((old_storage, new_storage)) = &storage_move {
        fs::rename(old_storage, new_storage).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(path, &new_path) {
        if let Some((old_storage, new_storage)) = &storage_move {
            let _ = fs::rename(new_storage, old_storage);
        }
        return Err(error.to_string());
    }
    Ok(new_path)
}

fn same_file(left: &Path, right: &Path) -> bool {
    let Ok(left) = fs::metadata(left) else {
        return false;
    };
    let Ok(right) = fs::metadata(right) else {
        return false;
    };
    left.dev() == right.dev() && left.ino() == right.ino()
}

/// Copies the config byte for byte and clones its disks with APFS copy-on-write, so the
/// duplicate takes no extra space until either VM writes to it.
pub fn duplicate_vm_files(path: &Path, document: &VmConfigDocument, name: &str) -> Result<PathBuf, String> {
    if document.storage_dir().is_some() {
        return Err("Virtual machines with a custom storage directory can't be duplicated.".to_owned());
    }
    let name = validate_vm_name(name)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let new_path = parent.join(format!("{name}.varmint"));
    let storage = implicit_storage_dir(path);
    let new_storage = parent.join(name);
    if new_path.exists() || new_storage.exists() {
        return Err(format!("A virtual machine named “{name}” already exists."));
    }

    let config = fs::read(path).map_err(|error| error.to_string())?;
    let _lock = lock_storage(&storage)?;
    if storage.is_dir()
        && let Err(error) = clone_storage(&storage, &new_storage)
    {
        let _ = fs::remove_dir_all(&new_storage);
        return Err(format!("The disks could not be cloned: {error}"));
    }
    if let Err(error) = write_new_config(&new_path, &config, name) {
        let _ = fs::remove_dir_all(&new_storage);
        return Err(error);
    }
    Ok(new_path)
}

fn clone_storage(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_name() == "vm.lock" || !entry.file_type()?.is_file() {
            continue;
        }
        let source = CString::new(entry.path().as_os_str().as_bytes())?;
        let destination = CString::new(to.join(entry.file_name()).as_os_str().as_bytes())?;
        if unsafe { libc::clonefile(source.as_ptr(), destination.as_ptr(), 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn is_managed_vm(path: &Path) -> bool {
    default_vms_directory()
        .ok()
        .is_some_and(|directory| path.parent() == Some(directory.as_path()))
}

/// Trashes the config before its storage: a config left without storage would quietly get
/// fresh disks at the next launch, while storage left without a config is just unused.
pub fn trash_managed_vm(path: &Path, document: &VmConfigDocument) -> Result<(), String> {
    let storage =
        Some(implicit_storage_dir(path)).filter(|storage| document.storage_dir().is_none() && storage.exists());
    let _lock = match &storage {
        Some(storage) => lock_storage(storage)?,
        None => None,
    };
    move_to_trash(path)?;
    match &storage {
        Some(storage) => move_to_trash(storage),
        None => Ok(()),
    }
}

fn move_to_trash(path: &Path) -> Result<(), String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|error| error.localizedDescription().to_string())
}

fn validate_vm_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name for the virtual machine.".to_owned());
    }
    if name == "." || name == ".." || name.contains(['/', ':']) {
        return Err("The name cannot contain a slash or colon.".to_owned());
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn suggests_unused_names_and_lists_configs_in_a_folder() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("varmint-names-test-{}-{unique}", std::process::id()));
        fs::create_dir_all(directory.join("My VM 2")).unwrap();
        assert_eq!(unused_vm_name(&directory), "My VM");
        create_vm_config_in("My VM", &directory).unwrap();
        fs::write(directory.join("notes.txt"), b"").unwrap();
        assert_eq!(unused_vm_name(&directory), "My VM 3");
        assert_eq!(vm_configs_in(&directory), vec![directory.join("My VM.varmint")]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn creates_minimal_vm_config_without_overwriting() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("varmint-create-test-{}-{unique}", std::process::id()));
        let path = create_vm_config_in("  Test VM  ", &directory).unwrap();
        assert_eq!(path, directory.join("Test VM.varmint"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "format_version = 2\n");
        assert!(
            create_vm_config_in("Test VM", &directory)
                .unwrap_err()
                .contains("already exists")
        );
        assert!(create_vm_config_in("bad/name", &directory).is_err());
        fs::create_dir(directory.join("Old VM")).unwrap();
        assert!(
            create_vm_config_in("Old VM", &directory)
                .unwrap_err()
                .contains("storage directory")
        );
        assert!(!directory.join("Old VM.varmint").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rename_moves_implicit_storage_but_not_explicit_storage() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("varmint-rename-test-{}-{unique}", std::process::id()));

        let implicit = create_vm_config_in("Implicit", &directory).unwrap();
        fs::create_dir(directory.join("Implicit")).unwrap();
        fs::write(directory.join("Implicit/data.raw"), b"disk").unwrap();
        let document = VmConfigDocument::open(&implicit).unwrap();
        let renamed = rename_vm_files(&implicit, &document, "Renamed").unwrap();
        assert_eq!(renamed, directory.join("Renamed.varmint"));
        assert!(!directory.join("Implicit").exists());
        assert_eq!(fs::read(directory.join("Renamed/data.raw")).unwrap(), b"disk");

        let explicit = directory.join("Explicit.varmint");
        fs::write(&explicit, "format_version = 2\nstorage_dir = \"shared\"\n").unwrap();
        fs::create_dir(directory.join("shared")).unwrap();
        let document = VmConfigDocument::open(&explicit).unwrap();
        rename_vm_files(&explicit, &document, "External").unwrap();
        assert!(directory.join("shared").is_dir());

        let fresh = create_vm_config_in("Fresh", &directory).unwrap();
        fs::create_dir(directory.join("Old Storage")).unwrap();
        let document = VmConfigDocument::open(&fresh).unwrap();
        assert!(rename_vm_files(&fresh, &document, "Old Storage").is_err());
        assert!(fresh.exists());

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rename_can_only_change_letter_case() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("varmint-case-rename-test-{}-{unique}", std::process::id()));

        let original = create_vm_config_in("Case Name", &directory).unwrap();
        fs::create_dir(directory.join("Case Name")).unwrap();
        let document = VmConfigDocument::open(&original).unwrap();
        let renamed = rename_vm_files(&original, &document, "case name").unwrap();

        assert_eq!(renamed, directory.join("case name.varmint"));
        let names = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "case name.varmint"));
        assert!(names.iter().any(|name| name == "case name"));

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn duplicate_copies_config_exactly_and_clones_disks() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("varmint-duplicate-test-{}-{unique}", std::process::id()));
        fs::create_dir_all(directory.join("Original")).unwrap();
        let config = "format_version = 2\n# memory_mib = 20000\nvcpus = 6 # note\n";
        let original = directory.join("Original.varmint");
        fs::write(&original, config).unwrap();
        fs::write(directory.join("Original/data.raw"), b"disk").unwrap();
        fs::write(directory.join("Original/vm.lock"), b"").unwrap();

        let document = VmConfigDocument::open(&original).unwrap();
        let copy = duplicate_vm_files(&original, &document, "Copy").unwrap();
        assert_eq!(copy, directory.join("Copy.varmint"));
        assert_eq!(fs::read_to_string(&copy).unwrap(), config);
        assert_eq!(fs::read(directory.join("Copy/data.raw")).unwrap(), b"disk");
        assert!(!directory.join("Copy/vm.lock").exists());
        assert!(duplicate_vm_files(&original, &document, "Copy").is_err());

        let explicit = directory.join("Explicit.varmint");
        fs::write(&explicit, "format_version = 2\nstorage_dir = \"shared\"\n").unwrap();
        let document = VmConfigDocument::open(&explicit).unwrap();
        assert!(duplicate_vm_files(&explicit, &document, "Other").is_err());
        assert!(!directory.join("Other.varmint").exists());

        fs::remove_dir_all(directory).unwrap();
    }
}
