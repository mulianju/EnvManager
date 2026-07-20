use crate::domain::environment::{EnvironmentScope, EnvironmentVariable};
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const BACKUP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupDocument {
    pub schema_version: u32,
    pub id: String,
    pub created_at_ms: u64,
    pub scope: EnvironmentScope,
    pub reason: String,
    pub variables: Vec<EnvironmentVariable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub id: String,
    pub created_at_ms: u64,
    pub scope: EnvironmentScope,
    pub reason: String,
    pub variable_count: usize,
}

impl From<&BackupDocument> for BackupSummary {
    fn from(document: &BackupDocument) -> Self {
        Self {
            id: document.id.clone(),
            created_at_ms: document.created_at_ms,
            scope: document.scope,
            reason: document.reason.clone(),
            variable_count: document.variables.len(),
        }
    }
}

#[derive(Debug)]
pub enum BackupError {
    PathUnavailable,
    InvalidId,
    Io(io::Error),
    InvalidJson(serde_json::Error),
    UnsupportedSchema(u32),
}

impl fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathUnavailable => formatter.write_str("Backup directory is unavailable."),
            Self::InvalidId => formatter.write_str("Backup ID is invalid."),
            Self::Io(error) => write!(formatter, "Backup file operation failed: {error}"),
            Self::InvalidJson(error) => write!(formatter, "Backup JSON is invalid: {error}"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "Backup schema {version} is not supported.")
            }
        }
    }
}

impl std::error::Error for BackupError {}

impl From<io::Error> for BackupError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct BackupStore {
    directory: PathBuf,
}

impl BackupStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn from_default_location() -> Result<Self, BackupError> {
        Ok(Self::new(default_backup_directory()?))
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn create(
        &self,
        scope: EnvironmentScope,
        reason: &str,
        variables: Vec<EnvironmentVariable>,
    ) -> Result<BackupSummary, BackupError> {
        fs::create_dir_all(&self.directory)?;
        let created_at_ms = current_time_ms();
        let scope_name = match scope {
            EnvironmentScope::User => "user",
            EnvironmentScope::System => "system",
        };
        let id = format!(
            "{created_at_ms}-{scope_name}-{}-{}.json",
            std::process::id(),
            unique_suffix()
        );
        let document = BackupDocument {
            schema_version: BACKUP_SCHEMA_VERSION,
            id: id.clone(),
            created_at_ms,
            scope,
            reason: reason.to_owned(),
            variables,
        };
        let serialized = serde_json::to_vec_pretty(&document).map_err(BackupError::InvalidJson)?;
        let temporary_path = self.directory.join(format!("{id}.tmp"));
        let destination = self.directory.join(&id);
        let mut file = File::create(&temporary_path)?;
        file.write_all(&serialized)?;
        file.sync_all()?;
        fs::rename(temporary_path, destination)?;

        Ok(BackupSummary::from(&document))
    }

    pub fn list(&self) -> Result<Vec<BackupSummary>, BackupError> {
        if !self.directory.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            let path = entry?.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            backups.push(BackupSummary::from(&self.read_document(&path)?));
        }
        backups.sort_by(|left, right| right.created_at_ms.cmp(&left.created_at_ms));
        Ok(backups)
    }

    pub fn load(&self, id: &str) -> Result<BackupDocument, BackupError> {
        if !is_safe_backup_id(id) {
            return Err(BackupError::InvalidId);
        }
        self.read_document(&self.directory.join(id))
    }

    fn read_document(&self, path: &Path) -> Result<BackupDocument, BackupError> {
        let serialized = fs::read_to_string(path)?;
        let document = serde_json::from_str::<BackupDocument>(&serialized)
            .map_err(BackupError::InvalidJson)?;
        if document.schema_version != BACKUP_SCHEMA_VERSION {
            return Err(BackupError::UnsupportedSchema(document.schema_version));
        }
        Ok(document)
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        % 1_000_000
}

fn is_safe_backup_id(id: &str) -> bool {
    !id.is_empty()
        && id.ends_with(".json")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && Path::new(id).file_name().and_then(|name| name.to_str()) == Some(id)
}

fn default_backup_directory() -> Result<PathBuf, BackupError> {
    #[cfg(windows)]
    {
        let app_data = env::var_os("APPDATA").ok_or(BackupError::PathUnavailable)?;
        return Ok(PathBuf::from(app_data).join("EnvManager").join("backups"));
    }

    #[cfg(not(windows))]
    {
        let home = env::var_os("HOME").ok_or(BackupError::PathUnavailable)?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("env-manager")
            .join("backups"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::environment::EnvironmentValueType;

    #[test]
    fn round_trips_and_lists_backups() {
        let directory = std::env::temp_dir().join(format!(
            "env-manager-backup-test-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let store = BackupStore::new(directory.clone());
        let variables = vec![EnvironmentVariable {
            name: "JAVA_HOME".to_owned(),
            value: r"C:\Java".to_owned(),
            value_type: EnvironmentValueType::String,
            scope: EnvironmentScope::User,
        }];

        let summary = store
            .create(EnvironmentScope::User, "beforeSet", variables.clone())
            .unwrap();
        let document = store.load(&summary.id).unwrap();

        assert_eq!(document.variables, variables);
        assert_eq!(store.list().unwrap(), vec![summary]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_path_traversal_ids() {
        let store = BackupStore::new(std::env::temp_dir());
        assert!(matches!(
            store.load("../backup.json"),
            Err(BackupError::InvalidId)
        ));
    }
}
