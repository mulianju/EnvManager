use crate::services::transfer_file::write_bytes_atomically;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(windows))]
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

const COMMAND_SHIM_SCHEMA_VERSION: u32 = 1;
const MAX_COMMAND_SHIMS_SIZE: u64 = 1024 * 1024;
const OWNERSHIP_PREFIX: &str = "rem envmanager-id:";
const SHELL_OWNERSHIP_PREFIX: &str = "# envmanager-id:";
#[cfg(windows)]
const COMMAND_SHIM_LOCK_TIMEOUT_MS: u32 = 10_000;
static SHIM_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandShimInput {
    pub id: Option<String>,
    pub command_name: String,
    pub executable: PathBuf,
    pub fixed_arguments: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandShimStatus {
    Ready,
    MissingExecutable,
    MissingTarget,
    NameConflict,
    ExternallyModified,
    MissingShim,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandShim {
    pub id: String,
    pub command_name: String,
    pub executable: PathBuf,
    pub fixed_arguments: Vec<String>,
    pub shim_path: PathBuf,
    pub status: CommandShimStatus,
    pub status_message: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandShimSnapshot {
    pub items: Vec<CommandShim>,
    pub managed_directory: PathBuf,
    pub path_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredCommandShim {
    id: String,
    command_name: String,
    executable: PathBuf,
    fixed_arguments: Vec<String>,
    content_sha256: String,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandShimDocument {
    schema_version: u32,
    items: Vec<StoredCommandShim>,
}

#[derive(Debug)]
pub enum CommandShimError {
    UnsupportedPlatform,
    PathUnavailable,
    InvalidCommandName(String),
    InvalidExecutable(String),
    MissingExecutable(PathBuf),
    InvalidFixedArgument(String),
    MissingTarget(PathBuf),
    DuplicateCommandName(String),
    ShimNotFound,
    NameConflict(PathBuf),
    ExternallyModified(PathBuf),
    Io(io::Error),
    InvalidJson(serde_json::Error),
    UnsupportedSchema(u32),
    InvalidDocument(String),
    FileTooLarge,
    LockTimeout,
    LockFailed(io::Error),
    TransactionRollbackFailed(String),
}

impl fmt::Display for CommandShimError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("Command Shims are only available on Windows.")
            }
            Self::PathUnavailable => formatter.write_str("Command Shim paths are unavailable."),
            Self::InvalidCommandName(message) => {
                write!(formatter, "Invalid command name: {message}")
            }
            Self::InvalidExecutable(message) => write!(formatter, "Invalid executable: {message}"),
            Self::MissingExecutable(path) => {
                write!(formatter, "Executable does not exist: {}", path.display())
            }
            Self::InvalidFixedArgument(message) => {
                write!(formatter, "Invalid fixed argument: {message}")
            }
            Self::MissingTarget(path) => {
                write!(
                    formatter,
                    "Fixed argument target does not exist: {}",
                    path.display()
                )
            }
            Self::DuplicateCommandName(name) => write!(formatter, "Command {name} already exists."),
            Self::ShimNotFound => formatter.write_str("Command Shim was not found."),
            Self::NameConflict(path) => write!(
                formatter,
                "A file not owned by EnvManager already uses this command name: {}",
                path.display()
            ),
            Self::ExternallyModified(path) => write!(
                formatter,
                "The managed Command Shim was modified outside EnvManager: {}",
                path.display()
            ),
            Self::Io(error) => write!(formatter, "Command Shim file operation failed: {error}"),
            Self::InvalidJson(error) => write!(formatter, "Command Shim JSON is invalid: {error}"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "Command Shim schema {version} is not supported.")
            }
            Self::InvalidDocument(message) => {
                write!(formatter, "Command Shim data is invalid: {message}")
            }
            Self::FileTooLarge => {
                formatter.write_str("Command Shim file is too large (maximum 1 MiB).")
            }
            Self::LockTimeout => {
                formatter.write_str("Timed out waiting for the Command Shim write lock.")
            }
            Self::LockFailed(error) => write!(formatter, "Command Shim write lock failed: {error}"),
            Self::TransactionRollbackFailed(message) => {
                write!(
                    formatter,
                    "Command Shim transaction rollback failed: {message}"
                )
            }
        }
    }
}

impl std::error::Error for CommandShimError {}

impl From<io::Error> for CommandShimError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct CommandShimStore {
    config_path: PathBuf,
    managed_directory: PathBuf,
}

impl CommandShimStore {
    pub fn new(config_path: PathBuf, managed_directory: PathBuf) -> Self {
        Self {
            config_path,
            managed_directory,
        }
    }

    pub fn from_default_locations() -> Result<Self, CommandShimError> {
        #[cfg(windows)]
        {
            let app_data = env::var_os("APPDATA").ok_or(CommandShimError::PathUnavailable)?;
            let local_app_data =
                env::var_os("LOCALAPPDATA").ok_or(CommandShimError::PathUnavailable)?;
            return Ok(Self::new(
                PathBuf::from(app_data)
                    .join("EnvManager")
                    .join("command-shims.json"),
                PathBuf::from(local_app_data).join("EnvManager").join("bin"),
            ));
        }

        #[cfg(not(windows))]
        {
            Ok(Self::new(PathBuf::new(), PathBuf::new()))
        }
    }

    pub fn managed_directory(&self) -> &Path {
        &self.managed_directory
    }

    pub fn snapshot(&self, path_ready: bool) -> Result<CommandShimSnapshot, CommandShimError> {
        self.snapshot_with_path(path_ready, &current_process_path_entries())
    }

    pub fn snapshot_with_path(
        &self,
        path_ready: bool,
        search_path: &[PathBuf],
    ) -> Result<CommandShimSnapshot, CommandShimError> {
        let _read_guard = acquire_command_shim_write_lock()?;
        let document = self.read_document()?;
        Ok(self.snapshot_from_document(&document, path_ready, search_path))
    }

    fn snapshot_from_document(
        &self,
        document: &CommandShimDocument,
        path_ready: bool,
        search_path: &[PathBuf],
    ) -> CommandShimSnapshot {
        let mut items = document
            .items
            .iter()
            .map(|item| self.to_public(item, search_path))
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.command_name
                .to_ascii_lowercase()
                .cmp(&right.command_name.to_ascii_lowercase())
                .then_with(|| left.command_name.cmp(&right.command_name))
        });
        CommandShimSnapshot {
            items,
            managed_directory: self.managed_directory.clone(),
            path_ready,
        }
    }

    pub fn validate(&self, input: &CommandShimInput) -> Result<(), CommandShimError> {
        validate_input(input.clone()).map(|_| ())
    }

    pub fn preflight_save(
        &self,
        input: &CommandShimInput,
        search_path: &[PathBuf],
    ) -> Result<(), CommandShimError> {
        let _write_guard = acquire_command_shim_write_lock()?;
        let normalized = validate_input(input.clone())?;
        let document = self.read_document()?;
        let existing = match normalized.id.as_deref() {
            Some(id) => Some(
                document
                    .items
                    .iter()
                    .find(|item| item.id == id)
                    .ok_or(CommandShimError::ShimNotFound)?,
            ),
            None => None,
        };
        if let Some(duplicate) = document.items.iter().find(|item| {
            Some(item.id.as_str()) != normalized.id.as_deref()
                && names_equal(&item.command_name, &normalized.command_name)
        }) {
            return Err(CommandShimError::DuplicateCommandName(
                duplicate.command_name.clone(),
            ));
        }
        if let Some(item) = existing {
            self.require_owned_content_for_update(item)?;
        }
        let destination = self.shim_path(&normalized.command_name);
        if existing.map(|item| self.shim_path(&item.command_name)) != Some(destination) {
            self.require_destination_available(&normalized.command_name)?;
        }
        if let Some(conflict) = find_path_conflict(
            &normalized.command_name,
            &self.managed_directory,
            search_path,
        ) {
            return Err(CommandShimError::NameConflict(conflict));
        }
        Ok(())
    }

    pub fn save(&self, input: CommandShimInput) -> Result<(), CommandShimError> {
        self.save_with_path(input, &current_process_path_entries(), false)
            .map(|_| ())
    }

    pub fn save_with_path(
        &self,
        input: CommandShimInput,
        search_path: &[PathBuf],
        path_ready: bool,
    ) -> Result<CommandShimSnapshot, CommandShimError> {
        let _write_guard = acquire_command_shim_write_lock()?;
        let normalized = validate_input(input)?;
        let mut document = self.read_document()?;
        let existing_index = match normalized.id.as_deref() {
            Some(id) => Some(
                document
                    .items
                    .iter()
                    .position(|item| item.id == id)
                    .ok_or(CommandShimError::ShimNotFound)?,
            ),
            None => None,
        };
        if let Some(existing) = document.items.iter().find(|item| {
            Some(item.id.as_str()) != normalized.id.as_deref()
                && names_equal(&item.command_name, &normalized.command_name)
        }) {
            return Err(CommandShimError::DuplicateCommandName(
                existing.command_name.clone(),
            ));
        }

        let old_document = document.clone();
        let old_item = existing_index.map(|index| document.items[index].clone());
        let old_shell_existed = match &old_item {
            Some(item) => self.require_owned_content_for_update(item)?,
            None => false,
        };

        let destination = self.shim_path(&normalized.command_name);
        let shell_destination = self.shell_shim_path(&normalized.command_name);
        let old_path = old_item
            .as_ref()
            .map(|item| self.shim_path(&item.command_name));
        let old_shell_path = old_item
            .as_ref()
            .map(|item| self.shell_shim_path(&item.command_name));
        if old_path.as_ref() != Some(&destination) {
            self.require_destination_available(&normalized.command_name)?;
        }
        if let Some(conflict) = find_path_conflict(
            &normalized.command_name,
            &self.managed_directory,
            search_path,
        ) {
            return Err(CommandShimError::NameConflict(conflict));
        }

        let now = now_ms();
        let id = old_item
            .as_ref()
            .map(|item| item.id.clone())
            .unwrap_or_else(|| create_shim_id(&normalized.command_name));
        let content = build_shim_content(&id, &normalized.executable, &normalized.fixed_arguments)?;
        let shell_content =
            build_shell_shim_content(&id, &normalized.executable, &normalized.fixed_arguments)?;
        let item = StoredCommandShim {
            id,
            command_name: normalized.command_name,
            executable: normalized.executable,
            fixed_arguments: normalized.fixed_arguments,
            content_sha256: content_hash(content.as_bytes()),
            created_at_ms: old_item.as_ref().map_or(now, |item| item.created_at_ms),
            updated_at_ms: now,
        };
        if let Some(index) = existing_index {
            document.items[index] = item;
        } else {
            document.items.push(item);
        }

        fs::create_dir_all(&self.managed_directory)?;
        write_bytes_atomically(&destination, content.as_bytes())?;
        if let Err(error) = write_bytes_atomically(&shell_destination, shell_content.as_bytes()) {
            let rollback = self.restore_after_failed_save(
                old_item.as_ref(),
                old_shell_existed,
                &destination,
                content.as_bytes(),
                &shell_destination,
                shell_content.as_bytes(),
            );
            return Err(with_rollback(error.into(), rollback));
        }
        if let Err(error) = self.write_document(&document) {
            let rollback = self.restore_after_failed_save(
                old_item.as_ref(),
                old_shell_existed,
                &destination,
                content.as_bytes(),
                &shell_destination,
                shell_content.as_bytes(),
            );
            return Err(with_rollback(error, rollback));
        }

        if let (Some(_), Some(old_path)) = (&old_item, old_path)
            && old_path != destination
            && let Err(remove_error) = fs::remove_file(&old_path)
        {
            let config_rollback = self.write_document(&old_document);
            let destination_rollback =
                remove_file_if_matches(&destination, content.as_bytes()).and(
                    remove_file_if_matches(&shell_destination, shell_content.as_bytes()),
                );
            let rollback = config_rollback.and(destination_rollback);
            return Err(with_rollback(CommandShimError::Io(remove_error), rollback));
        }
        if let (Some(old), Some(old_shell_path)) = (&old_item, old_shell_path)
            && old_shell_path != shell_destination
            && old_shell_existed
            && let Err(remove_error) = fs::remove_file(&old_shell_path)
        {
            let old_file_rollback = self.write_owned_item(old);
            let config_rollback = self.write_document(&old_document);
            let destination_rollback =
                remove_file_if_matches(&destination, content.as_bytes()).and(
                    remove_file_if_matches(&shell_destination, shell_content.as_bytes()),
                );
            let rollback = old_file_rollback
                .and(config_rollback)
                .and(destination_rollback);
            return Err(with_rollback(CommandShimError::Io(remove_error), rollback));
        }
        Ok(self.snapshot_from_document(&document, path_ready, search_path))
    }

    pub fn delete(&self, id: &str) -> Result<(), CommandShimError> {
        self.delete_with_path(id, false, &current_process_path_entries())
            .map(|_| ())
    }

    pub fn delete_with_path(
        &self,
        id: &str,
        path_ready: bool,
        search_path: &[PathBuf],
    ) -> Result<CommandShimSnapshot, CommandShimError> {
        let _write_guard = acquire_command_shim_write_lock()?;
        let mut document = self.read_document()?;
        let index = document
            .items
            .iter()
            .position(|item| item.id == id)
            .ok_or(CommandShimError::ShimNotFound)?;
        let item = document.items[index].clone();
        let path = self.shim_path(&item.command_name);
        let shell_path = self.shell_shim_path(&item.command_name);
        let shell_existed = self.require_owned_content_for_update(&item)?;
        fs::remove_file(&path)?;
        if shell_existed && let Err(error) = fs::remove_file(&shell_path) {
            return Err(with_rollback(error.into(), self.write_owned_item(&item)));
        }
        document.items.remove(index);
        if let Err(error) = self.write_document(&document) {
            let rollback = if shell_existed {
                self.write_owned_item(&item)
            } else {
                self.write_batch_item(&item)
            };
            return Err(with_rollback(error, rollback));
        }
        Ok(self.snapshot_from_document(&document, path_ready, search_path))
    }

    #[cfg(windows)]
    pub fn transaction<T, E>(&self, operation: impl FnOnce() -> Result<T, E>) -> Result<T, E>
    where
        E: From<CommandShimError>,
    {
        let _transaction_guard = acquire_command_shim_write_lock().map_err(E::from)?;
        operation()
    }

    fn to_public(&self, item: &StoredCommandShim, search_path: &[PathBuf]) -> CommandShim {
        let shim_path = self.shim_path(&item.command_name);
        let (status, status_message) = self.item_status(item, &shim_path, search_path);
        CommandShim {
            id: item.id.clone(),
            command_name: item.command_name.clone(),
            executable: item.executable.clone(),
            fixed_arguments: item.fixed_arguments.clone(),
            shim_path,
            status,
            status_message,
            created_at_ms: item.created_at_ms,
            updated_at_ms: item.updated_at_ms,
        }
    }

    fn item_status(
        &self,
        item: &StoredCommandShim,
        shim_path: &Path,
        search_path: &[PathBuf],
    ) -> (CommandShimStatus, Option<String>) {
        match fs::read(shim_path) {
            Ok(bytes) if content_hash(&bytes) != item.content_sha256 => {
                return (
                    CommandShimStatus::ExternallyModified,
                    Some(
                        "The generated file no longer matches the managed configuration."
                            .to_owned(),
                    ),
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return (
                    CommandShimStatus::MissingShim,
                    Some("The generated .cmd file is missing.".to_owned()),
                );
            }
            Err(error) => {
                return (
                    CommandShimStatus::ExternallyModified,
                    Some(format!("The generated file could not be read: {error}")),
                );
            }
            _ => {}
        }
        let shell_path = self.shell_shim_path(&item.command_name);
        let expected_shell =
            match build_shell_shim_content(&item.id, &item.executable, &item.fixed_arguments) {
                Ok(content) => content,
                Err(error) => {
                    return (
                        CommandShimStatus::ExternallyModified,
                        Some(format!(
                            "The Git Bash wrapper could not be validated: {error}"
                        )),
                    );
                }
            };
        match fs::read(&shell_path) {
            Ok(bytes) if bytes != expected_shell.as_bytes() => {
                return (
                    CommandShimStatus::ExternallyModified,
                    Some("The Git Bash wrapper was modified outside EnvManager.".to_owned()),
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return (
                    CommandShimStatus::MissingShim,
                    Some(
                        "The Git Bash wrapper is missing. Save the command to regenerate it."
                            .to_owned(),
                    ),
                );
            }
            Err(error) => {
                return (
                    CommandShimStatus::ExternallyModified,
                    Some(format!("The Git Bash wrapper could not be read: {error}")),
                );
            }
            _ => {}
        }
        if !item.executable.is_file() {
            return (
                CommandShimStatus::MissingExecutable,
                Some(format!(
                    "Executable is missing: {}",
                    item.executable.display()
                )),
            );
        }
        if !is_supported_executable(&item.executable) {
            return (
                CommandShimStatus::MissingExecutable,
                Some("Executable must be a Windows .exe or .com file.".to_owned()),
            );
        }
        if let Some(path) = first_missing_absolute_argument(&item.fixed_arguments) {
            return (
                CommandShimStatus::MissingTarget,
                Some(format!(
                    "Fixed argument target is missing: {}",
                    path.display()
                )),
            );
        }
        if let Some(path) =
            find_path_conflict(&item.command_name, &self.managed_directory, search_path)
        {
            return (
                CommandShimStatus::NameConflict,
                Some(format!(
                    "Another PATH entry also provides this command: {}",
                    path.display()
                )),
            );
        }
        (CommandShimStatus::Ready, None)
    }

    fn require_destination_available(&self, command_name: &str) -> Result<(), CommandShimError> {
        for conflict in [
            self.shell_shim_path(command_name),
            self.shim_path(command_name),
            self.managed_directory.join(format!("{command_name}.bat")),
            self.managed_directory.join(format!("{command_name}.exe")),
            self.managed_directory.join(format!("{command_name}.com")),
        ] {
            if conflict.exists() {
                return Err(CommandShimError::NameConflict(conflict));
            }
        }
        Ok(())
    }

    fn require_owned_content_for_update(
        &self,
        item: &StoredCommandShim,
    ) -> Result<bool, CommandShimError> {
        let path = self.shim_path(&item.command_name);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(CommandShimError::ExternallyModified(path));
            }
            Err(error) => return Err(error.into()),
        };
        let ownership = format!("{OWNERSHIP_PREFIX}{}", item.id);
        let has_ownership = String::from_utf8_lossy(&bytes)
            .lines()
            .any(|line| line.trim().eq_ignore_ascii_case(&ownership));
        if !has_ownership || content_hash(&bytes) != item.content_sha256 {
            return Err(CommandShimError::ExternallyModified(path));
        }
        let shell_path = self.shell_shim_path(&item.command_name);
        let shell_bytes = match fs::read(&shell_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let expected_shell =
            build_shell_shim_content(&item.id, &item.executable, &item.fixed_arguments)?;
        let ownership = format!("{SHELL_OWNERSHIP_PREFIX}{}", item.id);
        let has_ownership = String::from_utf8_lossy(&shell_bytes)
            .lines()
            .any(|line| line.trim() == ownership);
        if !has_ownership || shell_bytes != expected_shell.as_bytes() {
            return Err(CommandShimError::ExternallyModified(shell_path));
        }
        Ok(true)
    }

    fn write_owned_item(&self, item: &StoredCommandShim) -> Result<(), CommandShimError> {
        self.write_batch_item(item)?;
        let content = build_shell_shim_content(&item.id, &item.executable, &item.fixed_arguments)?;
        write_bytes_atomically(
            &self.shell_shim_path(&item.command_name),
            content.as_bytes(),
        )?;
        Ok(())
    }

    fn write_batch_item(&self, item: &StoredCommandShim) -> Result<(), CommandShimError> {
        let content = build_shim_content(&item.id, &item.executable, &item.fixed_arguments)?;
        write_bytes_atomically(&self.shim_path(&item.command_name), content.as_bytes())?;
        Ok(())
    }

    fn restore_after_failed_save(
        &self,
        old_item: Option<&StoredCommandShim>,
        old_shell_existed: bool,
        destination: &Path,
        destination_content: &[u8],
        shell_destination: &Path,
        shell_destination_content: &[u8],
    ) -> Result<(), CommandShimError> {
        if let Some(old) = old_item
            && self.shim_path(&old.command_name) == destination
        {
            self.write_batch_item(old)?;
            if old_shell_existed {
                let content =
                    build_shell_shim_content(&old.id, &old.executable, &old.fixed_arguments)?;
                write_bytes_atomically(shell_destination, content.as_bytes())?;
            } else {
                remove_file_if_matches(shell_destination, shell_destination_content)?;
            }
            return Ok(());
        }
        remove_file_if_matches(destination, destination_content)?;
        remove_file_if_matches(shell_destination, shell_destination_content)
    }

    fn shim_path(&self, command_name: &str) -> PathBuf {
        self.managed_directory.join(format!("{command_name}.cmd"))
    }

    fn shell_shim_path(&self, command_name: &str) -> PathBuf {
        self.managed_directory.join(command_name)
    }

    fn read_document(&self) -> Result<CommandShimDocument, CommandShimError> {
        let mut file = match File::open(&self.config_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(empty_document()),
            Err(error) => return Err(error.into()),
        };
        if file.metadata()?.len() > MAX_COMMAND_SHIMS_SIZE {
            return Err(CommandShimError::FileTooLarge);
        }
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_COMMAND_SHIMS_SIZE + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_COMMAND_SHIMS_SIZE {
            return Err(CommandShimError::FileTooLarge);
        }
        let document = serde_json::from_slice::<CommandShimDocument>(&bytes)
            .map_err(CommandShimError::InvalidJson)?;
        validate_document(&document)?;
        Ok(document)
    }

    fn write_document(&self, document: &CommandShimDocument) -> Result<(), CommandShimError> {
        let bytes = serde_json::to_vec_pretty(document).map_err(CommandShimError::InvalidJson)?;
        if bytes.len() as u64 > MAX_COMMAND_SHIMS_SIZE {
            return Err(CommandShimError::FileTooLarge);
        }
        if let Some(directory) = self.config_path.parent() {
            fs::create_dir_all(directory)?;
        }
        write_bytes_atomically(&self.config_path, &bytes)?;
        Ok(())
    }
}

fn validate_input(mut input: CommandShimInput) -> Result<CommandShimInput, CommandShimError> {
    input.command_name = input.command_name.trim().to_owned();
    validate_command_name(&input.command_name)?;
    if !input.executable.is_absolute() {
        return Err(CommandShimError::InvalidExecutable(
            "Use an absolute file path.".to_owned(),
        ));
    }
    if !input.executable.is_file() {
        return Err(CommandShimError::MissingExecutable(input.executable));
    }
    if !is_supported_executable(&input.executable) {
        return Err(CommandShimError::InvalidExecutable(
            "Select a Windows .exe or .com file. For scripts, select the runtime executable and add the script path as a fixed argument."
                .to_owned(),
        ));
    }
    for argument in &input.fixed_arguments {
        if argument.contains(['\0', '\r', '\n']) {
            return Err(CommandShimError::InvalidFixedArgument(
                "NUL and line breaks are not allowed.".to_owned(),
            ));
        }
    }
    if let Some(path) = first_missing_absolute_argument(&input.fixed_arguments) {
        return Err(CommandShimError::MissingTarget(path));
    }
    Ok(input)
}

fn validate_document(document: &CommandShimDocument) -> Result<(), CommandShimError> {
    if document.schema_version != COMMAND_SHIM_SCHEMA_VERSION {
        return Err(CommandShimError::UnsupportedSchema(document.schema_version));
    }
    for (index, item) in document.items.iter().enumerate() {
        validate_command_name(&item.command_name)?;
        if item.id.is_empty() || item.id.contains(['\0', '\r', '\n']) {
            return Err(CommandShimError::InvalidDocument(
                "A Command Shim has an invalid ownership id.".to_owned(),
            ));
        }
        if item.content_sha256.len() != 64
            || !item
                .content_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(CommandShimError::InvalidDocument(
                "A Command Shim has an invalid content checksum.".to_owned(),
            ));
        }
        if document.items[..index]
            .iter()
            .any(|existing| names_equal(&existing.command_name, &item.command_name))
        {
            return Err(CommandShimError::InvalidDocument(format!(
                "Duplicate command name {}.",
                item.command_name
            )));
        }
    }
    Ok(())
}

fn validate_command_name(name: &str) -> Result<(), CommandShimError> {
    if name.is_empty() {
        return Err(CommandShimError::InvalidCommandName(
            "A command name is required.".to_owned(),
        ));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(CommandShimError::InvalidCommandName(
            "Names cannot end with a dot or space.".to_owned(),
        ));
    }
    if name.chars().any(|character| {
        !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    }) {
        return Err(CommandShimError::InvalidCommandName(
            "Use only letters, numbers, dots, hyphens, and underscores.".to_owned(),
        ));
    }
    if [".cmd", ".bat", ".exe", ".com"]
        .iter()
        .any(|extension| name.to_ascii_lowercase().ends_with(extension))
    {
        return Err(CommandShimError::InvalidCommandName(
            "Enter the command without a Windows executable extension.".to_owned(),
        ));
    }
    let base = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || base
            .strip_prefix("COM")
            .or_else(|| base.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            });
    if reserved {
        return Err(CommandShimError::InvalidCommandName(
            "Windows reserves this device name.".to_owned(),
        ));
    }
    Ok(())
}

fn first_missing_absolute_argument(arguments: &[String]) -> Option<PathBuf> {
    arguments.iter().find_map(|argument| {
        let path = PathBuf::from(argument);
        (path.is_absolute() && !path.exists()).then_some(path)
    })
}

fn build_shim_content(
    id: &str,
    executable: &Path,
    fixed_arguments: &[String],
) -> Result<String, CommandShimError> {
    let executable = executable.to_str().ok_or_else(|| {
        CommandShimError::InvalidExecutable("The path must be valid Unicode.".to_owned())
    })?;
    let mut command = escape_batch_argument(executable);
    for argument in fixed_arguments {
        command.push(' ');
        command.push_str(&escape_batch_argument(argument));
    }
    command.push_str(" %*");
    Ok(format!(
        "@echo off\r\nsetlocal DisableDelayedExpansion\r\nrem EnvManager command shim v1\r\n{OWNERSHIP_PREFIX}{id}\r\n{command}\r\nexit /b %errorlevel%\r\n"
    ))
}

fn build_shell_shim_content(
    id: &str,
    executable: &Path,
    fixed_arguments: &[String],
) -> Result<String, CommandShimError> {
    let executable = executable.to_str().ok_or_else(|| {
        CommandShimError::InvalidExecutable("The path must be valid Unicode.".to_owned())
    })?;
    let mut command = format!(
        "exec {}",
        escape_shell_argument(&windows_path_for_shell(executable))
    );
    for argument in fixed_arguments {
        command.push(' ');
        let value = if is_windows_absolute_path(argument) {
            windows_path_for_shell(argument)
        } else {
            argument.clone()
        };
        command.push_str(&escape_shell_argument(&value));
    }
    command.push_str(" \"$@\"");
    Ok(format!(
        "#!/usr/bin/env bash\n# EnvManager command shim v1\n{SHELL_OWNERSHIP_PREFIX}{id}\n{command}\n"
    ))
}

fn escape_batch_argument(value: &str) -> String {
    let escaped_percent = value.replace('%', "%%");
    let mut result = String::from("\"");
    let mut backslashes = 0usize;
    for character in escaped_percent.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        if character == '"' {
            result.push('"');
            result.push_str(&"\\".repeat(backslashes * 2 + 1));
            result.push('^');
            result.push('"');
            result.push('"');
        } else {
            result.push_str(&"\\".repeat(backslashes));
            result.push(character);
        }
        backslashes = 0;
    }
    result.push_str(&"\\".repeat(backslashes * 2));
    result.push('"');
    result
}

fn escape_shell_argument(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn windows_path_for_shell(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    let bytes = normalized.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        return format!(
            "/{}/{}",
            (bytes[0] as char).to_ascii_lowercase(),
            &normalized[3..]
        );
    }
    normalized
}

fn is_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || value.starts_with(r"\\")
        || value.starts_with("//")
}

fn is_supported_executable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
        })
}

fn current_process_path_entries() -> Vec<PathBuf> {
    env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect())
        .unwrap_or_default()
}

fn find_path_conflict(
    command_name: &str,
    managed_directory: &Path,
    search_path: &[PathBuf],
) -> Option<PathBuf> {
    for directory in search_path {
        if path_identity(&directory) == path_identity(managed_directory) {
            continue;
        }
        for candidate in [
            directory.join(command_name),
            directory.join(format!("{command_name}.cmd")),
            directory.join(format!("{command_name}.bat")),
            directory.join(format!("{command_name}.exe")),
            directory.join(format!("{command_name}.com")),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn path_identity(path: &Path) -> String {
    path.to_string_lossy()
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn names_equal(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn create_shim_id(command_name: &str) -> String {
    let counter = SHIM_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = format!(
        "{command_name}:{}:{}:{counter}",
        std::process::id(),
        now_ms()
    );
    content_hash(seed.as_bytes())[..24].to_owned()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn empty_document() -> CommandShimDocument {
    CommandShimDocument {
        schema_version: COMMAND_SHIM_SCHEMA_VERSION,
        items: Vec::new(),
    }
}

fn remove_file_if_exists(path: &Path) -> Result<(), CommandShimError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn remove_file_if_matches(path: &Path, expected: &[u8]) -> Result<(), CommandShimError> {
    match fs::read(path) {
        Ok(bytes) if bytes == expected => remove_file_if_exists(path),
        Ok(_) => Err(CommandShimError::ExternallyModified(path.to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn with_rollback(
    error: CommandShimError,
    rollback: Result<(), CommandShimError>,
) -> CommandShimError {
    match rollback {
        Ok(()) => error,
        Err(rollback_error) => CommandShimError::TransactionRollbackFailed(format!(
            "{error}; rollback error: {rollback_error}"
        )),
    }
}

#[cfg(windows)]
struct CommandShimWriteGuard {
    handle: HANDLE,
    owns_mutex: bool,
}

#[cfg(windows)]
impl Drop for CommandShimWriteGuard {
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
fn acquire_command_shim_write_lock() -> Result<CommandShimWriteGuard, CommandShimError> {
    let name = "Local\\EnvManager.CommandShims"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(CommandShimError::LockFailed(io::Error::last_os_error()));
    }
    let mut guard = CommandShimWriteGuard {
        handle,
        owns_mutex: false,
    };
    match unsafe { WaitForSingleObject(handle, COMMAND_SHIM_LOCK_TIMEOUT_MS) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => {
            guard.owns_mutex = true;
            Ok(guard)
        }
        WAIT_TIMEOUT => Err(CommandShimError::LockTimeout),
        WAIT_FAILED => Err(CommandShimError::LockFailed(io::Error::last_os_error())),
        status => Err(CommandShimError::LockFailed(io::Error::other(format!(
            "Unexpected wait status {status}."
        )))),
    }
}

#[cfg(not(windows))]
struct CommandShimWriteGuard {
    _guard: MutexGuard<'static, ()>,
}

#[cfg(not(windows))]
fn acquire_command_shim_write_lock() -> Result<CommandShimWriteGuard, CommandShimError> {
    static COMMAND_SHIM_WRITE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = COMMAND_SHIM_WRITE_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| {
            CommandShimError::LockFailed(io::Error::other("Command Shim write lock is poisoned."))
        })?;
    Ok(CommandShimWriteGuard { _guard: guard })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(windows)]
    use std::io::Write;
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    #[cfg(windows)]
    use std::process::{Command, Stdio};

    struct Harness {
        root: PathBuf,
        store: CommandShimStore,
        executable: PathBuf,
    }

    impl Harness {
        fn new() -> Self {
            let root = env::temp_dir().join(format!(
                "env-manager-command-shim-{}-{}",
                std::process::id(),
                SHIM_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            let executable = root.join("runtime.exe");
            fs::write(&executable, b"test runtime").unwrap();
            let store = CommandShimStore::new(
                root.join("config").join("command-shims.json"),
                root.join("managed bin"),
            );
            Self {
                root,
                store,
                executable,
            }
        }

        fn input(&self, name: &str) -> CommandShimInput {
            CommandShimInput {
                id: None,
                command_name: name.to_owned(),
                executable: self.executable.clone(),
                fixed_arguments: vec!["--mode=test".to_owned()],
            }
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn command_name_validation_rejects_reserved_and_unsafe_names() {
        assert!(validate_command_name("sharedev").is_ok());
        for name in [
            "", "CON", "com1.txt", "a/b", "a\\b", "bad:name", "bad name", "bad&name", "tool.cmd",
            "trail.",
        ] {
            assert!(
                validate_command_name(name).is_err(),
                "{name} must be rejected"
            );
        }
    }

    #[test]
    fn batch_content_quotes_fixed_arguments_and_forwards_runtime_arguments() {
        let executable = Path::new(r"C:\Program Files\Node\node.exe");
        let content = build_shim_content(
            "owned-id",
            executable,
            &[r"D:\a folder\tool.js".to_owned(), "100%".to_owned()],
        )
        .unwrap();

        assert!(content.contains("setlocal DisableDelayedExpansion"));
        assert!(content.contains("rem envmanager-id:owned-id"));
        assert!(
            content
                .contains(r#""C:\Program Files\Node\node.exe" "D:\a folder\tool.js" "100%%" %*"#)
        );
        assert!(content.ends_with("exit /b %errorlevel%\r\n"));
    }

    #[test]
    fn shell_content_quotes_fixed_arguments_and_forwards_runtime_arguments() {
        let executable = Path::new(r"C:\Program Files\Node\node.exe");
        let content = build_shell_shim_content(
            "owned-id",
            executable,
            &[
                r"D:\a folder\tool.js".to_owned(),
                "fixed value".to_owned(),
                "say'hi".to_owned(),
            ],
        )
        .unwrap();

        assert!(content.starts_with("#!/usr/bin/env bash\n"));
        assert!(content.contains("# envmanager-id:owned-id"));
        assert!(content.contains("'/c/Program Files/Node/node.exe'"));
        assert!(content.contains("'/d/a folder/tool.js'"));
        assert!(content.contains("'fixed value'"));
        assert!(content.contains("'say'\"'\"'hi'"));
        assert!(content.ends_with(" \"$@\"\n"));
    }

    #[test]
    fn create_edit_rename_and_delete_preserve_owned_lifecycle() {
        let harness = Harness::new();
        harness.store.save(harness.input("sharedev")).unwrap();
        let created = harness.store.snapshot(true).unwrap().items.remove(0);
        assert_eq!(created.status, CommandShimStatus::Ready);
        assert!(created.shim_path.is_file());
        let created_shell_path = harness.store.shell_shim_path("sharedev");
        assert!(created_shell_path.is_file());

        let mut edited = harness.input("sharedev-next");
        edited.id = Some(created.id.clone());
        edited.fixed_arguments.push("--verbose".to_owned());
        harness.store.save(edited).unwrap();
        let renamed = harness.store.snapshot(true).unwrap().items.remove(0);
        assert_eq!(renamed.command_name, "sharedev-next");
        assert!(!created.shim_path.exists());
        assert!(!created_shell_path.exists());
        assert!(renamed.shim_path.is_file());
        let renamed_shell_path = harness.store.shell_shim_path("sharedev-next");
        assert!(renamed_shell_path.is_file());

        harness.store.delete(&renamed.id).unwrap();
        assert!(!renamed.shim_path.exists());
        assert!(!renamed_shell_path.exists());
        assert!(harness.store.snapshot(true).unwrap().items.is_empty());
        assert!(harness.executable.exists());
    }

    #[test]
    fn external_files_are_never_overwritten_or_deleted() {
        let harness = Harness::new();
        fs::create_dir_all(harness.store.managed_directory()).unwrap();
        let external = harness.store.managed_directory().join("sharedev.cmd");
        fs::write(&external, b"@echo external").unwrap();
        assert!(matches!(
            harness.store.save(harness.input("sharedev")),
            Err(CommandShimError::NameConflict(_))
        ));
        assert_eq!(fs::read(&external).unwrap(), b"@echo external");

        fs::remove_file(&external).unwrap();
        harness.store.save(harness.input("sharedev")).unwrap();
        let item = harness.store.snapshot(true).unwrap().items.remove(0);
        fs::write(&item.shim_path, b"@echo modified").unwrap();
        assert_eq!(
            harness.store.snapshot(true).unwrap().items[0].status,
            CommandShimStatus::ExternallyModified
        );
        assert!(matches!(
            harness.store.delete(&item.id),
            Err(CommandShimError::ExternallyModified(_))
        ));
        assert_eq!(fs::read(&item.shim_path).unwrap(), b"@echo modified");

        fs::remove_file(&item.shim_path).unwrap();
        harness
            .store
            .write_owned_item(&StoredCommandShim {
                id: item.id.clone(),
                command_name: item.command_name.clone(),
                executable: item.executable.clone(),
                fixed_arguments: item.fixed_arguments.clone(),
                content_sha256: content_hash(
                    build_shim_content(&item.id, &item.executable, &item.fixed_arguments)
                        .unwrap()
                        .as_bytes(),
                ),
                created_at_ms: item.created_at_ms,
                updated_at_ms: item.updated_at_ms,
            })
            .unwrap();
        let shell_path = harness.store.shell_shim_path("sharedev");
        fs::write(&shell_path, b"#!/usr/bin/env bash\necho modified\n").unwrap();
        assert_eq!(
            harness.store.snapshot(true).unwrap().items[0].status,
            CommandShimStatus::ExternallyModified
        );
        assert!(matches!(
            harness.store.delete(&item.id),
            Err(CommandShimError::ExternallyModified(_))
        ));
        assert_eq!(
            fs::read(&shell_path).unwrap(),
            b"#!/usr/bin/env bash\necho modified\n"
        );
    }

    #[test]
    fn external_shell_entry_is_never_overwritten() {
        let harness = Harness::new();
        fs::create_dir_all(harness.store.managed_directory()).unwrap();
        let external = harness.store.shell_shim_path("sharedev");
        fs::write(&external, b"#!/usr/bin/env bash\necho external\n").unwrap();

        assert!(matches!(
            harness.store.save(harness.input("sharedev")),
            Err(CommandShimError::NameConflict(_))
        ));
        assert_eq!(
            fs::read(&external).unwrap(),
            b"#!/usr/bin/env bash\necho external\n"
        );
        assert!(!harness.store.shim_path("sharedev").exists());
    }

    #[test]
    fn save_rejects_missing_executable_and_absolute_target() {
        let harness = Harness::new();
        let mut missing_executable = harness.input("missing-runtime");
        missing_executable.executable = harness.root.join("missing.exe");
        assert!(matches!(
            harness.store.save(missing_executable),
            Err(CommandShimError::MissingExecutable(_))
        ));

        let mut missing_target = harness.input("missing-target");
        missing_target.fixed_arguments =
            vec![harness.root.join("missing.js").display().to_string()];
        assert!(matches!(
            harness.store.save(missing_target),
            Err(CommandShimError::MissingTarget(_))
        ));

        let unsupported = harness.root.join("script.ps1");
        fs::write(&unsupported, b"Write-Output test").unwrap();
        let mut unsupported_executable = harness.input("unsupported-runtime");
        unsupported_executable.executable = unsupported;
        assert!(matches!(
            harness.store.save(unsupported_executable),
            Err(CommandShimError::InvalidExecutable(_))
        ));
    }

    #[test]
    fn preflight_rejects_a_command_already_available_on_the_live_path() {
        let harness = Harness::new();
        let external_directory = harness.root.join("external bin");
        fs::create_dir_all(&external_directory).unwrap();
        let external = external_directory.join("sharedev.exe");
        fs::write(&external, b"external command").unwrap();

        assert!(matches!(
            harness
                .store
                .preflight_save(&harness.input("sharedev"), &[external_directory]),
            Err(CommandShimError::NameConflict(path)) if path == external
        ));
        assert!(!harness.store.managed_directory().exists());
    }

    #[cfg(windows)]
    #[test]
    fn outer_transaction_lock_supports_nested_preflight_and_committed_snapshot() {
        let harness = Harness::new();
        let input = harness.input("transaction-test");

        let snapshot = harness
            .store
            .transaction::<_, CommandShimError>(|| {
                harness.store.preflight_save(&input, &[])?;
                harness.store.save_with_path(input, &[], true)
            })
            .unwrap();

        assert!(snapshot.path_ready);
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].command_name, "transaction-test");
        assert!(!snapshot.items[0].id.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn generated_shim_preserves_streams_arguments_spaces_and_exit_code() {
        let harness = Harness::new();
        let script_directory = harness.root.join("脚本 files");
        fs::create_dir_all(&script_directory).unwrap();
        let script = script_directory.join("shim target.ps1");
        fs::write(
            &script,
            concat!(
                "param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Values)\r\n",
                "$line = [Console]::In.ReadLine()\r\n",
                "[Console]::Out.WriteLine(($Values -join '|'))\r\n",
                "[Console]::Out.WriteLine(('stdin=' + $line))\r\n",
                "[Console]::Error.WriteLine('stderr-ok')\r\n",
                "exit 7\r\n"
            ),
        )
        .unwrap();
        let powershell = PathBuf::from(env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let content = build_shim_content(
            "execution-test",
            &powershell,
            &[
                "-NoLogo".to_owned(),
                "-NoProfile".to_owned(),
                "-ExecutionPolicy".to_owned(),
                "Bypass".to_owned(),
                "-File".to_owned(),
                script.display().to_string(),
                "fixed value".to_owned(),
                "fixed&value".to_owned(),
                "100%".to_owned(),
                "say\"hi".to_owned(),
            ],
        )
        .unwrap();
        fs::create_dir_all(harness.store.managed_directory()).unwrap();
        let shim = harness.store.managed_directory().join("execution-test.cmd");
        fs::write(&shim, content).unwrap();

        let test_path = env::join_paths(
            std::iter::once(harness.store.managed_directory().to_owned())
                .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
        )
        .unwrap();
        let mut child = Command::new("cmd.exe")
            .arg("/d")
            .arg("/c")
            .raw_arg(r#"execution-test "hello world" "left&right" "100%""#)
            .env("PATH", test_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"hello-input\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert_eq!(
            output.status.code(),
            Some(7),
            "stdout={stdout}; stderr={stderr}"
        );
        assert!(
            stdout.contains("fixed value|fixed&value|100%|say\"hi|hello world|left&right|100%"),
            "{stdout}"
        );
        assert!(stdout.contains("stdin=hello-input"), "{stdout}");
        assert!(stderr.contains("stderr-ok"), "{stderr}");
    }

    #[cfg(windows)]
    #[test]
    fn generated_shell_shim_runs_from_git_bash_path() {
        let Some(bash) = find_git_bash() else {
            return;
        };
        let harness = Harness::new();
        let script_directory = harness.root.join("Git Bash 脚本 files");
        fs::create_dir_all(&script_directory).unwrap();
        let script = script_directory.join("shell target.ps1");
        fs::write(
            &script,
            concat!(
                "param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Values)\r\n",
                "$line = [Console]::In.ReadLine()\r\n",
                "[Console]::Out.WriteLine(($Values -join '|'))\r\n",
                "[Console]::Out.WriteLine(('stdin=' + $line))\r\n",
                "[Console]::Error.WriteLine('stderr-ok')\r\n",
                "exit 7\r\n"
            ),
        )
        .unwrap();
        let powershell = PathBuf::from(env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let content = build_shell_shim_content(
            "git-bash-execution-test",
            &powershell,
            &[
                "-NoLogo".to_owned(),
                "-NoProfile".to_owned(),
                "-ExecutionPolicy".to_owned(),
                "Bypass".to_owned(),
                "-File".to_owned(),
                script.display().to_string(),
                "fixed value".to_owned(),
                "fixed&value".to_owned(),
                "say'hi".to_owned(),
            ],
        )
        .unwrap();
        fs::create_dir_all(harness.store.managed_directory()).unwrap();
        let shim = harness.store.shell_shim_path("git-bash-execution-test");
        fs::write(&shim, content).unwrap();
        let managed_directory =
            windows_path_for_shell(harness.store.managed_directory().to_str().unwrap());
        let command = format!(
            "PATH={}:\"$PATH\"; git-bash-execution-test 'hello world' 'left&right' '100%'",
            escape_shell_argument(&managed_directory)
        );
        let mut child = Command::new(bash)
            .arg("-lc")
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"hello-input\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert_eq!(
            output.status.code(),
            Some(7),
            "stdout={stdout}; stderr={stderr}"
        );
        assert!(
            stdout.contains("fixed value|fixed&value|say'hi|hello world|left&right|100%"),
            "{stdout}"
        );
        assert!(stdout.contains("stdin=hello-input"), "{stdout}");
        assert!(stderr.contains("stderr-ok"), "{stderr}");
    }

    #[cfg(windows)]
    fn find_git_bash() -> Option<PathBuf> {
        let output = Command::new("git").arg("--exec-path").output().ok()?;
        if !output.status.success() {
            return None;
        }
        let exec_path = String::from_utf8(output.stdout).ok()?;
        let root = PathBuf::from(exec_path.trim())
            .ancestors()
            .nth(3)?
            .to_owned();
        let bash = root.join("bin").join("bash.exe");
        bash.is_file().then_some(bash)
    }
}
