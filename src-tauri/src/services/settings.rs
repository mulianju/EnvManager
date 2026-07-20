use crate::domain::environment::{
    EnvironmentScope, EnvironmentValidationError, EnvironmentVariable, compare_variable_names,
    validate_variable_name, variable_names_equal,
};
use crate::services::transfer_file::write_bytes_atomically;
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::sync::{Mutex, MutexGuard, OnceLock};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

const SETTINGS_SCHEMA_VERSION: u32 = 1;
const MAX_SETTINGS_SIZE: u64 = 1024 * 1024;
#[cfg(windows)]
const SETTINGS_LOCK_TIMEOUT_MS: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteKey {
    pub scope: EnvironmentScope,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsDocument {
    schema_version: u32,
    favorites: Vec<FavoriteKey>,
}

#[derive(Debug)]
pub enum SettingsError {
    PathUnavailable,
    Io(io::Error),
    InvalidJson(serde_json::Error),
    UnsupportedSchema(u32),
    Validation(EnvironmentValidationError),
    DuplicateFavorite,
    FileTooLarge,
    LockTimeout,
    LockFailed(io::Error),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathUnavailable => formatter.write_str("Settings path is unavailable."),
            Self::Io(error) => write!(formatter, "Settings file operation failed: {error}"),
            Self::InvalidJson(error) => write!(formatter, "Settings JSON is invalid: {error}"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "Settings schema {version} is not supported.")
            }
            Self::Validation(error) => {
                write!(formatter, "Favorite variable name is invalid: {error}")
            }
            Self::DuplicateFavorite => {
                formatter.write_str("Settings contain a duplicate favorite variable.")
            }
            Self::FileTooLarge => {
                formatter.write_str("Settings file is too large (maximum 1 MiB).")
            }
            Self::LockTimeout => {
                formatter.write_str("Timed out waiting for the settings write lock.")
            }
            Self::LockFailed(error) => {
                write!(formatter, "Settings write lock failed: {error}")
            }
        }
    }
}

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidJson(error) => Some(error),
            Self::Validation(error) => Some(error),
            Self::LockFailed(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for SettingsError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn from_default_location() -> Result<Self, SettingsError> {
        Ok(Self::new(default_settings_path()?))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> Result<Vec<FavoriteKey>, SettingsError> {
        self.list_unlocked()
    }

    fn list_unlocked(&self) -> Result<Vec<FavoriteKey>, SettingsError> {
        Ok(self.read_document()?.favorites)
    }

    pub fn toggle(&self, favorite: FavoriteKey) -> Result<Vec<FavoriteKey>, SettingsError> {
        let _write_guard = acquire_settings_write_lock()?;
        validate_variable_name(&favorite.name).map_err(SettingsError::Validation)?;
        let mut document = self.read_document()?;
        if let Some(index) = document.favorites.iter().position(|existing| {
            existing.scope == favorite.scope && variable_names_equal(&existing.name, &favorite.name)
        }) {
            document.favorites.remove(index);
        } else {
            document.favorites.push(favorite);
        }
        sort_favorites(&mut document.favorites);
        self.write_document(&document)?;
        Ok(document.favorites)
    }

    pub fn reconcile(
        &self,
        user_variables: &[EnvironmentVariable],
        system_variables: &[EnvironmentVariable],
    ) -> Result<Vec<FavoriteKey>, SettingsError> {
        let _write_guard = acquire_settings_write_lock()?;
        let document = self.read_document()?;
        let mut favorites = document
            .favorites
            .iter()
            .filter_map(|favorite| {
                let variables = match favorite.scope {
                    EnvironmentScope::User => user_variables,
                    EnvironmentScope::System => system_variables,
                };
                variables
                    .iter()
                    .find(|variable| variable_names_equal(&variable.name, &favorite.name))
                    .map(|variable| FavoriteKey {
                        scope: favorite.scope,
                        name: variable.name.clone(),
                    })
            })
            .collect::<Vec<_>>();
        sort_favorites(&mut favorites);

        if favorites != document.favorites {
            self.write_document(&SettingsDocument {
                schema_version: SETTINGS_SCHEMA_VERSION,
                favorites: favorites.clone(),
            })?;
        }
        Ok(favorites)
    }

    fn read_document(&self) -> Result<SettingsDocument, SettingsError> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(empty_document());
            }
            Err(error) => return Err(error.into()),
        };
        if file.metadata()?.len() > MAX_SETTINGS_SIZE {
            return Err(SettingsError::FileTooLarge);
        }

        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_SETTINGS_SIZE + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_SETTINGS_SIZE {
            return Err(SettingsError::FileTooLarge);
        }

        let mut document = serde_json::from_slice::<SettingsDocument>(&bytes)
            .map_err(SettingsError::InvalidJson)?;
        validate_document(&document)?;
        sort_favorites(&mut document.favorites);
        Ok(document)
    }

    fn write_document(&self, document: &SettingsDocument) -> Result<(), SettingsError> {
        let bytes = serde_json::to_vec_pretty(document).map_err(SettingsError::InvalidJson)?;
        if bytes.len() as u64 > MAX_SETTINGS_SIZE {
            return Err(SettingsError::FileTooLarge);
        }
        if let Some(directory) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(directory)?;
        }
        write_bytes_atomically(&self.path, &bytes)?;
        Ok(())
    }
}

#[cfg(windows)]
struct SettingsWriteGuard {
    handle: HANDLE,
    owns_mutex: bool,
}

#[cfg(windows)]
impl Drop for SettingsWriteGuard {
    fn drop(&mut self) {
        unsafe {
            if self.owns_mutex {
                ReleaseMutex(self.handle);
            }
            CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
fn acquire_settings_write_lock() -> Result<SettingsWriteGuard, SettingsError> {
    let name = "Local\\EnvManager.Settings"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(SettingsError::LockFailed(io::Error::last_os_error()));
    }

    let mut guard = SettingsWriteGuard {
        handle,
        owns_mutex: false,
    };
    match unsafe { WaitForSingleObject(handle, SETTINGS_LOCK_TIMEOUT_MS) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => {
            guard.owns_mutex = true;
            Ok(guard)
        }
        WAIT_TIMEOUT => Err(SettingsError::LockTimeout),
        WAIT_FAILED => Err(SettingsError::LockFailed(io::Error::last_os_error())),
        status => Err(SettingsError::LockFailed(io::Error::other(format!(
            "Unexpected wait status {status}."
        )))),
    }
}

#[cfg(not(windows))]
struct SettingsWriteGuard {
    _guard: MutexGuard<'static, ()>,
}

#[cfg(not(windows))]
fn acquire_settings_write_lock() -> Result<SettingsWriteGuard, SettingsError> {
    static SETTINGS_WRITE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = SETTINGS_WRITE_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| {
            SettingsError::LockFailed(io::Error::other("Settings write lock is poisoned."))
        })?;
    Ok(SettingsWriteGuard { _guard: guard })
}

fn empty_document() -> SettingsDocument {
    SettingsDocument {
        schema_version: SETTINGS_SCHEMA_VERSION,
        favorites: Vec::new(),
    }
}

fn validate_document(document: &SettingsDocument) -> Result<(), SettingsError> {
    if document.schema_version != SETTINGS_SCHEMA_VERSION {
        return Err(SettingsError::UnsupportedSchema(document.schema_version));
    }
    for (index, favorite) in document.favorites.iter().enumerate() {
        validate_variable_name(&favorite.name).map_err(SettingsError::Validation)?;
        if document.favorites[..index].iter().any(|existing| {
            existing.scope == favorite.scope && variable_names_equal(&existing.name, &favorite.name)
        }) {
            return Err(SettingsError::DuplicateFavorite);
        }
    }
    Ok(())
}

fn sort_favorites(favorites: &mut [FavoriteKey]) {
    favorites.sort_by(|left, right| {
        scope_order(left.scope)
            .cmp(&scope_order(right.scope))
            .then_with(|| compare_variable_names(&left.name, &right.name))
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn scope_order(scope: EnvironmentScope) -> u8 {
    match scope {
        EnvironmentScope::User => 0,
        EnvironmentScope::System => 1,
    }
}

fn default_settings_path() -> Result<PathBuf, SettingsError> {
    #[cfg(windows)]
    {
        let app_data = env::var_os("APPDATA").ok_or(SettingsError::PathUnavailable)?;
        return Ok(PathBuf::from(app_data)
            .join("EnvManager")
            .join("settings.json"));
    }

    #[cfg(not(windows))]
    {
        let home = env::var_os("HOME").ok_or(SettingsError::PathUnavailable)?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("env-manager")
            .join("settings.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::{FavoriteKey, MAX_SETTINGS_SIZE, SettingsStore};
    use crate::domain::environment::{
        EnvironmentScope, EnvironmentValueType, EnvironmentVariable, compare_variable_names,
    };
    use serde_json::{Value, json};
    use std::cmp::Ordering;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    const SECRET_VALUE: &str = "real-secret-must-never-be-persisted";

    #[test]
    fn missing_settings_file_lists_empty_without_creating_any_path() {
        let directory = TempDirectory::new("missing");
        let settings_directory = directory.path().join("nested");
        let store = SettingsStore::new(settings_directory.join("settings.json"));

        assert_eq!(store.list().unwrap(), Vec::<FavoriteKey>::new());
        assert!(!settings_directory.exists());
    }

    #[test]
    fn toggle_uses_registry_identity_and_keeps_scopes_independent() {
        let directory = TempDirectory::new("identity");
        let store = SettingsStore::new(directory.path().join("settings.json"));

        assert_eq!(
            store
                .toggle(favorite(EnvironmentScope::User, "ÄPFEL"))
                .unwrap(),
            vec![favorite(EnvironmentScope::User, "ÄPFEL")]
        );
        assert!(
            store
                .toggle(favorite(EnvironmentScope::User, "äpfel"))
                .unwrap()
                .is_empty()
        );

        store
            .toggle(favorite(EnvironmentScope::User, "Path"))
            .unwrap();
        assert_eq!(
            store
                .toggle(favorite(EnvironmentScope::System, "PATH"))
                .unwrap(),
            vec![
                favorite(EnvironmentScope::User, "Path"),
                favorite(EnvironmentScope::System, "PATH"),
            ]
        );

        store.toggle(favorite(EnvironmentScope::User, "Σ")).unwrap();
        let favorites = store.toggle(favorite(EnvironmentScope::User, "ς")).unwrap();
        if compare_variable_names("Σ", "ς") == Ordering::Equal {
            assert!(!favorites.contains(&favorite(EnvironmentScope::User, "Σ")));
            assert!(!favorites.contains(&favorite(EnvironmentScope::User, "ς")));
        } else {
            assert!(favorites.contains(&favorite(EnvironmentScope::User, "Σ")));
            assert!(favorites.contains(&favorite(EnvironmentScope::User, "ς")));
        }
    }

    #[test]
    fn persisted_document_contains_only_favorite_identity_and_round_trips() {
        let directory = TempDirectory::new("roundtrip");
        let path = directory.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        store
            .toggle(favorite(EnvironmentScope::User, "API_TOKEN"))
            .unwrap();

        store
            .reconcile(
                &[variable(EnvironmentScope::User, "API_TOKEN", SECRET_VALUE)],
                &[],
            )
            .unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(document["schemaVersion"], 1);
        assert_eq!(document["favorites"].as_array().unwrap().len(), 1);
        let persisted_favorite = document["favorites"][0].as_object().unwrap();
        assert_eq!(persisted_favorite.len(), 2);
        assert_eq!(persisted_favorite["scope"], "user");
        assert_eq!(persisted_favorite["name"], "API_TOKEN");
        assert!(!persisted_favorite.contains_key("value"));
        assert!(!String::from_utf8_lossy(&bytes).contains(SECRET_VALUE));

        assert_eq!(
            SettingsStore::new(path).list().unwrap(),
            vec![favorite(EnvironmentScope::User, "API_TOKEN")]
        );
    }

    #[test]
    fn list_has_deterministic_scope_then_registry_name_order() {
        let directory = TempDirectory::new("order");
        let path = directory.path().join("settings.json");
        write_document(
            &path,
            json!({
                "schemaVersion": 1,
                "favorites": [
                    { "scope": "system", "name": "A_SYSTEM" },
                    { "scope": "user", "name": "z_user" },
                    { "scope": "system", "name": "Z_SYSTEM" },
                    { "scope": "user", "name": "A_user" },
                    { "scope": "user", "name": "ς" },
                    { "scope": "user", "name": "Σ" }
                ]
            }),
        );
        let store = SettingsStore::new(path);

        let actual = store.list().unwrap();
        let mut expected = vec![
            favorite(EnvironmentScope::System, "A_SYSTEM"),
            favorite(EnvironmentScope::User, "z_user"),
            favorite(EnvironmentScope::System, "Z_SYSTEM"),
            favorite(EnvironmentScope::User, "A_user"),
            favorite(EnvironmentScope::User, "ς"),
            favorite(EnvironmentScope::User, "Σ"),
        ];
        expected.sort_by(compare_favorites);

        assert_eq!(actual, expected);
        assert!(
            actual[..4]
                .iter()
                .all(|favorite| favorite.scope == EnvironmentScope::User)
        );
    }

    #[test]
    fn duplicate_registry_identity_in_one_scope_is_rejected() {
        let directory = TempDirectory::new("duplicate");
        let path = directory.path().join("settings.json");
        write_document(
            &path,
            json!({
                "schemaVersion": 1,
                "favorites": [
                    { "scope": "user", "name": "Path" },
                    { "scope": "user", "name": "PATH" }
                ]
            }),
        );

        assert_error_contains(SettingsStore::new(path).list(), &["duplicate", "favorite"]);
    }

    #[test]
    fn malformed_unsupported_and_oversized_files_are_rejected_without_overwrite() {
        for (label, original, expected) in [
            ("corrupt", b"{not-json".to_vec(), vec!["json"]),
            (
                "schema",
                br#"{"schemaVersion":2,"favorites":[]}"#.to_vec(),
                vec!["schema", "2"],
            ),
            ("oversized", vec![b' '; 1024 * 1024 + 1], vec!["large"]),
        ] {
            let directory = TempDirectory::new(label);
            let path = directory.path().join("settings.json");
            std::fs::write(&path, &original).unwrap();
            let store = SettingsStore::new(path.clone());

            assert_error_contains(store.list(), &expected);
            assert_error_contains(
                store.toggle(favorite(EnvironmentScope::User, "NEW_VALUE")),
                &expected,
            );
            assert_error_contains(store.reconcile(&[], &[]), &expected);
            assert_eq!(std::fs::read(path).unwrap(), original);
        }
    }

    #[test]
    fn invalid_favorite_names_are_rejected_without_changing_existing_file() {
        let directory = TempDirectory::new("invalid-name");
        let path = directory.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        store
            .toggle(favorite(EnvironmentScope::User, "KEEP_ME"))
            .unwrap();
        let original = std::fs::read(&path).unwrap();

        for invalid in ["", "HAS=EQUALS", "HAS\0NUL"] {
            assert_error_contains(
                store.toggle(favorite(EnvironmentScope::User, invalid)),
                &["name"],
            );
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }

        assert_eq!(
            store
                .toggle(favorite(EnvironmentScope::User, "AFTER_ERROR"))
                .unwrap(),
            vec![
                favorite(EnvironmentScope::User, "AFTER_ERROR"),
                favorite(EnvironmentScope::User, "KEEP_ME"),
            ]
        );
    }

    #[test]
    fn invalid_favorites_loaded_from_disk_are_rejected_without_overwrite() {
        let directory = TempDirectory::new("invalid-on-disk");
        let path = directory.path().join("settings.json");
        let original =
            br#"{"schemaVersion":1,"favorites":[{"scope":"user","name":"BAD\u0000NAME"}]}"#;
        std::fs::write(&path, original).unwrap();
        let store = SettingsStore::new(path.clone());

        assert_error_contains(store.list(), &["name"]);
        assert_error_contains(store.reconcile(&[], &[]), &["name"]);
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn reconcile_removes_missing_entries_and_adopts_registry_display_names() {
        let directory = TempDirectory::new("reconcile");
        let path = directory.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        store
            .toggle(favorite(EnvironmentScope::User, "path"))
            .unwrap();
        store
            .toggle(favorite(EnvironmentScope::System, "PATH"))
            .unwrap();
        store
            .toggle(favorite(EnvironmentScope::System, "MISSING"))
            .unwrap();

        let reconciled = store
            .reconcile(
                &[variable(EnvironmentScope::User, "Path", SECRET_VALUE)],
                &[variable(EnvironmentScope::System, "Path", "system value")],
            )
            .unwrap();

        assert_eq!(
            reconciled,
            vec![
                favorite(EnvironmentScope::User, "Path"),
                favorite(EnvironmentScope::System, "Path"),
            ]
        );
        assert_eq!(store.list().unwrap(), reconciled);
        let serialized = std::fs::read_to_string(path).unwrap();
        assert!(!serialized.contains("MISSING"));
        assert!(!serialized.contains(SECRET_VALUE));
    }

    #[test]
    fn reconcile_without_changes_preserves_document_bytes() {
        let directory = TempDirectory::new("reconcile-no-change");
        let path = directory.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        store
            .toggle(favorite(EnvironmentScope::User, "JAVA_HOME"))
            .unwrap();
        let before = std::fs::read(&path).unwrap();

        let favorites = store
            .reconcile(
                &[variable(EnvironmentScope::User, "JAVA_HOME", r"C:\Java")],
                &[],
            )
            .unwrap();

        assert_eq!(
            favorites,
            vec![favorite(EnvironmentScope::User, "JAVA_HOME")]
        );
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn successful_toggle_atomically_replaces_target_without_temp_files() {
        let directory = TempDirectory::new("atomic");
        let path = directory.path().join("settings.json");
        write_document(
            &path,
            json!({
                "schemaVersion": 1,
                "favorites": [{ "scope": "user", "name": "OLD" }]
            }),
        );
        let old_bytes = std::fs::read(&path).unwrap();
        let store = SettingsStore::new(path.clone());

        store
            .toggle(favorite(EnvironmentScope::User, "NEW"))
            .unwrap();

        assert_ne!(std::fs::read(&path).unwrap(), old_bytes);
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")
        }));
    }

    #[test]
    fn oversized_serialized_update_preserves_file_and_releases_write_lock() {
        let directory = TempDirectory::new("oversized-write");
        let path = directory.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        store
            .toggle(favorite(EnvironmentScope::User, "KEEP"))
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        let oversized_name = "A".repeat(MAX_SETTINGS_SIZE as usize);

        assert_error_contains(
            store.toggle(favorite(EnvironmentScope::User, &oversized_name)),
            &["large"],
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            store
                .toggle(favorite(EnvironmentScope::User, "NEXT"))
                .unwrap(),
            vec![
                favorite(EnvironmentScope::User, "KEEP"),
                favorite(EnvironmentScope::User, "NEXT"),
            ]
        );
    }

    #[test]
    fn concurrent_toggles_preserve_both_updates() {
        for iteration in 0..24 {
            let directory = TempDirectory::new(&format!("concurrent-{iteration}"));
            let path = directory.path().join("settings.json");
            let stores = [
                Arc::new(SettingsStore::new(path.clone())),
                Arc::new(SettingsStore::new(path.clone())),
            ];
            let barrier = Arc::new(Barrier::new(3));
            let workers = stores
                .into_iter()
                .zip(["LEFT", "RIGHT"])
                .map(|(store, name)| {
                    let barrier = Arc::clone(&barrier);
                    thread::spawn(move || {
                        barrier.wait();
                        store
                            .toggle(favorite(EnvironmentScope::User, name))
                            .unwrap();
                    })
                })
                .collect::<Vec<_>>();

            barrier.wait();
            for worker in workers {
                worker.join().unwrap();
            }
            assert_eq!(
                SettingsStore::new(path).list().unwrap(),
                vec![
                    favorite(EnvironmentScope::User, "LEFT"),
                    favorite(EnvironmentScope::User, "RIGHT"),
                ]
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn default_location_is_appdata_env_manager_settings_without_writing() {
        let app_data = std::env::var_os("APPDATA").expect("APPDATA must be available on Windows");
        let expected = PathBuf::from(app_data)
            .join("EnvManager")
            .join("settings.json");

        let store = SettingsStore::from_default_location().unwrap();

        assert_eq!(store.path(), expected.as_path());
    }

    fn favorite(scope: EnvironmentScope, name: &str) -> FavoriteKey {
        FavoriteKey {
            scope,
            name: name.to_owned(),
        }
    }

    fn variable(scope: EnvironmentScope, name: &str, value: &str) -> EnvironmentVariable {
        EnvironmentVariable {
            name: name.to_owned(),
            value: value.to_owned(),
            value_type: EnvironmentValueType::String,
            scope,
        }
    }

    fn compare_favorites(left: &FavoriteKey, right: &FavoriteKey) -> Ordering {
        scope_order(left.scope)
            .cmp(&scope_order(right.scope))
            .then_with(|| compare_variable_names(&left.name, &right.name))
            .then_with(|| left.name.cmp(&right.name))
    }

    fn scope_order(scope: EnvironmentScope) -> u8 {
        match scope {
            EnvironmentScope::User => 0,
            EnvironmentScope::System => 1,
        }
    }

    fn write_document(path: &Path, document: Value) {
        std::fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    }

    fn assert_error_contains<T, E: std::fmt::Display>(result: Result<T, E>, expected: &[&str]) {
        let message = match result {
            Ok(_) => panic!("expected settings operation to fail"),
            Err(error) => error.to_string().to_lowercase(),
        };
        for fragment in expected {
            assert!(
                message.contains(&fragment.to_lowercase()),
                "expected error '{message}' to contain '{fragment}'"
            );
        }
    }

    struct TempDirectory {
        path: PathBuf,
    }

    impl TempDirectory {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "env-manager-settings-{label}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
