use super::{NeptuneBackend, VmConfigFile, validate_config_file};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Item, value};

const MEMORY_MIB: &str = "memory_mib";
const VCPUS: &str = "vcpus";
const DATA_DISK_CAPACITY_GIB: &str = "data_disk_capacity_gib";
const NEPTUNE_BACKEND: &str = "neptune_backend";
const DUALSENSE_PASSTHROUGH: &str = "dualsense_passthrough";
const STORAGE_DIR: &str = "storage_dir";
const HOST_ENVIRONMENT: &str = "host_environment";

#[derive(Debug, Clone)]
pub struct VmConfigDocument {
    document: DocumentMut,
}

impl VmConfigDocument {
    pub fn open(path: &Path) -> Result<Self, String> {
        let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
        Self::parse(&source)
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        let config: VmConfigFile = toml::from_str(source).map_err(|error| error.to_string())?;
        validate_config_file(&config)?;
        let document = source
            .parse()
            .map_err(|error: toml_edit::TomlError| error.to_string())?;
        Ok(Self { document })
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let temporary = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            if let Ok(metadata) = fs::metadata(path) {
                file.set_permissions(metadata.permissions())?;
            }
            file.write_all(self.to_string().as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    pub fn memory_mib(&self) -> Option<u64> {
        self.unsigned_integer(MEMORY_MIB)
    }

    pub fn set_memory_mib(&mut self, value: Option<u64>) {
        self.set_unsigned_integer(MEMORY_MIB, value);
    }

    pub fn vcpus(&self) -> Option<usize> {
        self.unsigned_integer(VCPUS).and_then(|value| value.try_into().ok())
    }

    pub fn set_vcpus(&mut self, value: Option<usize>) {
        self.set_unsigned_integer(VCPUS, value.map(|value| value as u64));
    }

    pub fn data_disk_capacity_gib(&self) -> Option<u64> {
        self.unsigned_integer(DATA_DISK_CAPACITY_GIB)
    }

    pub fn set_data_disk_capacity_gib(&mut self, value: Option<u64>) {
        self.set_unsigned_integer(DATA_DISK_CAPACITY_GIB, value);
    }

    pub fn neptune_backend(&self) -> Option<NeptuneBackend> {
        match self.document.get(NEPTUNE_BACKEND)?.as_str()? {
            "dxmt" => Some(NeptuneBackend::Dxmt),
            "d3dmetal" => Some(NeptuneBackend::D3dmetal),
            _ => None,
        }
    }

    pub fn set_neptune_backend(&mut self, backend: Option<NeptuneBackend>) {
        if backend.is_some() && backend == self.neptune_backend() {
            return;
        }
        let backend = backend.map(|backend| match backend {
            NeptuneBackend::Dxmt => "dxmt",
            NeptuneBackend::D3dmetal => "d3dmetal",
        });
        self.set_value(NEPTUNE_BACKEND, backend.map(value));
    }

    pub fn dualsense_passthrough(&self) -> bool {
        self.document
            .get(DUALSENSE_PASSTHROUGH)
            .and_then(Item::as_bool)
            .unwrap_or(false)
    }

    /// Off is the default, so it is stored by removing the key.
    pub fn set_dualsense_passthrough(&mut self, enabled: bool) {
        if enabled == self.dualsense_passthrough() {
            return;
        }
        self.set_value(DUALSENSE_PASSTHROUGH, enabled.then(|| value(true)));
    }

    pub fn storage_dir(&self) -> Option<PathBuf> {
        self.document.get(STORAGE_DIR)?.as_str().map(PathBuf::from)
    }

    pub fn host_environment(&self, name: &str) -> Option<&str> {
        self.document.get(HOST_ENVIRONMENT)?.get(name)?.as_str()
    }

    pub fn set_host_environment(&mut self, name: &str, value: Option<&str>) {
        if value.is_some() && value == self.host_environment(name) {
            return;
        }
        let Some(value) = value else {
            self.remove_key(Some(HOST_ENVIRONMENT), name);
            let empty = self
                .document
                .get(HOST_ENVIRONMENT)
                .and_then(Item::as_table_like)
                .is_some_and(|table| table.is_empty());
            if empty {
                self.document.remove(HOST_ENVIRONMENT);
            }
            return;
        };
        let table = self
            .document
            .entry(HOST_ENVIRONMENT)
            .or_insert_with(toml_edit::table)
            .as_table_like_mut()
            .expect("validated host_environment is a table");
        let mut replacement = toml_edit::Value::from(value);
        if let Some(current) = table.get(name).and_then(Item::as_value) {
            replacement.decor_mut().clone_from(current.decor());
        }
        table.insert(name, Item::Value(replacement));
    }

    fn unsigned_integer(&self, key: &str) -> Option<u64> {
        self.document.get(key)?.as_integer()?.try_into().ok()
    }

    fn set_unsigned_integer(&mut self, key: &str, value: Option<u64>) {
        if value.is_some() && value == self.unsigned_integer(key) {
            return;
        }
        let value = value.map(|value| {
            let value = i64::try_from(value).unwrap_or_else(|_| panic!("{key} is too large for TOML"));
            toml_edit::value(value)
        });
        self.set_value(key, value);
    }

    fn set_value(&mut self, key: &str, replacement: Option<Item>) {
        let Some(mut replacement) = replacement else {
            self.remove_key(None, key);
            return;
        };

        let Some(current) = self.document.get_mut(key) else {
            self.document.insert(key, replacement);
            return;
        };

        if let (Some(current), Some(replacement)) = (current.as_value(), replacement.as_value_mut()) {
            replacement.decor_mut().clone_from(current.decor());
        }
        *current = replacement;
    }

    /// Removes the key's line but keeps comments around it. `table` names a root table; keys of
    /// an inline table share one line, so they are removed through `toml_edit` instead.
    fn remove_key(&mut self, table: Option<&str>, key: &str) {
        if let Some(table) = table
            && let Some(inline) = self.document.get_mut(table).and_then(Item::as_inline_table_mut)
        {
            inline.remove(key);
            return;
        }
        let source = self.document.to_string();
        let Ok(document) = toml_edit::Document::parse(source.clone()) else {
            return;
        };
        let (key, item) = match table {
            Some(table) => {
                let Some(table) = document.get(table).and_then(Item::as_table) else {
                    return;
                };
                (table.key(key), table.get(key))
            }
            None => (document.key(key), document.get(key)),
        };
        let Some(span) = key.and_then(|key| key.span()) else {
            return;
        };
        let Some(item_span) = item.and_then(Item::span) else {
            return;
        };
        let line_start = source[..span.start].rfind('\n').map_or(0, |index| index + 1);
        let line_end = source[item_span.end..]
            .find('\n')
            .map_or(source.len(), |index| item_span.end + index + 1);
        let suffix = &source[item_span.end..line_end];
        let replacement = suffix
            .find('#')
            .map(|comment| {
                let newline = if suffix.ends_with('\n') { "\n" } else { "" };
                format!("{}{}", suffix[comment..].trim_end_matches(['\r', '\n']), newline)
            })
            .unwrap_or_default();

        let mut edited = source;
        edited.replace_range(line_start..line_end, &replacement);
        self.document = edited
            .parse()
            .expect("removing a single-line key must leave valid TOML");
    }
}

impl std::fmt::Display for VmConfigDocument {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.document.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"format_version = 2

# Guest RAM. Defaults to half of host memory.
memory_mib = 20000 # Keep this exact value.
vcpus = 8
data_disk_capacity_gib = 160
neptune_backend = "d3dmetal"

[host_environment]
WINEDEBUG = "-all"
"#;

    #[test]
    fn unchanged_document_round_trips_exactly() {
        let document = VmConfigDocument::parse(SOURCE).unwrap();

        assert_eq!(document.to_string(), SOURCE);
    }

    #[test]
    fn reads_explicit_values_without_applying_defaults() {
        let document = VmConfigDocument::parse(SOURCE).unwrap();

        assert_eq!(document.memory_mib(), Some(20000));
        assert_eq!(document.vcpus(), Some(8));
        assert_eq!(document.data_disk_capacity_gib(), Some(160));
        assert_eq!(document.neptune_backend(), Some(NeptuneBackend::D3dmetal));
        assert_eq!(document.storage_dir(), None);
    }

    #[test]
    fn reads_storage_directory_for_disk_status() {
        let document = VmConfigDocument::parse("format_version = 2\nstorage_dir = \"disks/test\"\n").unwrap();

        assert_eq!(document.storage_dir(), Some(PathBuf::from("disks/test")));
    }

    #[test]
    fn changing_one_value_preserves_other_values_and_comments() {
        let mut document = VmConfigDocument::parse(SOURCE).unwrap();

        document.set_vcpus(Some(10));

        let expected = SOURCE.replacen("vcpus = 8", "vcpus = 10", 1);
        assert_eq!(document.to_string(), expected);
    }

    #[test]
    fn auto_removes_only_the_selected_key() {
        let mut document = VmConfigDocument::parse(SOURCE).unwrap();

        document.set_memory_mib(None);

        assert_eq!(document.memory_mib(), None);
        assert_eq!(
            document.to_string(),
            SOURCE.replacen(
                "memory_mib = 20000 # Keep this exact value.\n",
                "# Keep this exact value.\n",
                1
            )
        );
    }

    #[test]
    fn setting_an_auto_value_adds_only_that_key() {
        let source = "format_version = 2\n\n# Keep this note.\n";
        let mut document = VmConfigDocument::parse(source).unwrap();

        document.set_neptune_backend(Some(NeptuneBackend::Dxmt));

        assert_eq!(
            document.to_string(),
            "format_version = 2\nneptune_backend = \"dxmt\"\n\n# Keep this note.\n"
        );
    }

    #[test]
    fn setting_the_current_value_keeps_its_formatting() {
        let source = "format_version = 2\nmemory_mib = 20_000\nneptune_backend = 'dxmt'\n";
        let mut document = VmConfigDocument::parse(source).unwrap();

        document.set_memory_mib(Some(20000));
        document.set_neptune_backend(Some(NeptuneBackend::Dxmt));

        assert_eq!(document.to_string(), source);
    }

    #[test]
    fn new_keys_stay_in_the_root_table() {
        let source = "format_version = 2\n\n[host_environment]\nWINEDEBUG = \"-all\"\n";
        let mut document = VmConfigDocument::parse(source).unwrap();

        document.set_memory_mib(Some(20000));

        let saved = document.to_string();
        assert_eq!(VmConfigDocument::parse(&saved).unwrap().memory_mib(), Some(20000));
        assert!(saved.find("memory_mib").unwrap() < saved.find("[host_environment]").unwrap());
    }

    #[test]
    fn edits_host_environment_without_touching_other_variables() {
        let mut document = VmConfigDocument::parse(SOURCE).unwrap();

        document.set_host_environment("D3DMETAL_FRAMEWORK_PATH", Some("/D3DMetal.framework"));
        assert_eq!(
            document.host_environment("D3DMETAL_FRAMEWORK_PATH"),
            Some("/D3DMetal.framework")
        );
        assert_eq!(
            document.to_string(),
            SOURCE.replacen(
                "WINEDEBUG = \"-all\"\n",
                "WINEDEBUG = \"-all\"\nD3DMETAL_FRAMEWORK_PATH = \"/D3DMetal.framework\"\n",
                1
            )
        );

        document.set_host_environment("D3DMETAL_FRAMEWORK_PATH", None);
        assert_eq!(document.to_string(), SOURCE);
    }

    #[test]
    fn removing_the_last_host_variable_removes_the_table() {
        let source = "format_version = 2\n";
        let mut document = VmConfigDocument::parse(source).unwrap();

        document.set_host_environment("D3DMETAL_FRAMEWORK_PATH", Some("/D3DMetal.framework"));
        assert!(VmConfigDocument::parse(&document.to_string()).is_ok());
        document.set_host_environment("D3DMETAL_FRAMEWORK_PATH", None);

        assert_eq!(document.to_string(), source);
    }

    #[test]
    fn removing_a_host_variable_ignores_hashes_inside_its_value() {
        let source = r#"format_version = 2

[host_environment]
D3DMETAL_FRAMEWORK_PATH = "/Volumes/Games#2/D3DMetal.framework" # Keep this note.
WINEDEBUG = "-all"
"#;
        let mut document = VmConfigDocument::parse(source).unwrap();

        document.set_host_environment("D3DMETAL_FRAMEWORK_PATH", None);

        assert_eq!(
            document.to_string(),
            "format_version = 2\n\n[host_environment]\n# Keep this note.\nWINEDEBUG = \"-all\"\n"
        );
    }

    #[test]
    fn removes_a_multiline_host_variable() {
        let source = r#"format_version = 2

[host_environment]
SCRIPT = """
first
second
""" # Keep this note.
WINEDEBUG = "-all"
"#;
        let mut document = VmConfigDocument::parse(source).unwrap();

        document.set_host_environment("SCRIPT", None);

        assert_eq!(
            document.to_string(),
            "format_version = 2\n\n[host_environment]\n# Keep this note.\nWINEDEBUG = \"-all\"\n"
        );
    }

    #[test]
    fn opens_and_saves_an_edited_document() {
        let path = std::env::temp_dir().join(format!("varmint-config-{}.varmint", uuid::Uuid::new_v4()));
        fs::write(&path, SOURCE).unwrap();

        let mut document = VmConfigDocument::open(&path).unwrap();
        document.set_data_disk_capacity_gib(Some(256));
        document.save(&path).unwrap();

        let saved = fs::read_to_string(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(
            saved,
            SOURCE.replacen("data_disk_capacity_gib = 160", "data_disk_capacity_gib = 256", 1)
        );
    }

    #[test]
    fn dualsense_passthrough_is_off_unless_stored() {
        let source = "format_version = 2\n";
        let mut document = VmConfigDocument::parse(source).unwrap();
        assert!(!document.dualsense_passthrough());

        document.set_dualsense_passthrough(true);
        assert_eq!(
            document.to_string(),
            "format_version = 2\ndualsense_passthrough = true\n"
        );
        assert!(
            VmConfigDocument::parse(&document.to_string())
                .unwrap()
                .dualsense_passthrough()
        );

        document.set_dualsense_passthrough(false);
        assert_eq!(document.to_string(), source);
    }

    #[test]
    fn rejects_values_the_vm_loader_would_reject() {
        let unknown = "format_version = 2\ngui_name = \"Test\"\n";
        let invalid = "format_version = 2\nvcpus = 0\n";

        assert!(VmConfigDocument::parse(unknown).is_err());
        assert!(VmConfigDocument::parse(invalid).is_err());
    }
}
