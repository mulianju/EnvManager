use crate::domain::environment::{
    EnvironmentScope, EnvironmentValidationError, EnvironmentVariable, EnvironmentVariableInput,
    duplicate_path_entry_indexes, variable_names_equal,
};
use crate::platform::{EnvironmentStore, EnvironmentStoreError};
use crate::services::backup::{BackupError, BackupStore, BackupSummary};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub user_variables: Vec<EnvironmentVariable>,
    pub system_variables: Vec<EnvironmentVariable>,
    pub is_elevated: bool,
    pub backups: Vec<BackupSummary>,
    pub backup_directory: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathEntryStatus {
    pub value: String,
    pub expanded_value: String,
    pub exists: bool,
    pub duplicate: bool,
}

#[derive(Debug)]
pub enum EnvironmentServiceError {
    Validation(EnvironmentValidationError),
    ElevationRequired,
    VariableAlreadyExists(String),
    VariableNotFound(String),
    Store(EnvironmentStoreError),
    Backup(BackupError),
}

impl fmt::Display for EnvironmentServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::ElevationRequired => {
                formatter.write_str("Administrator permission is required for system variables.")
            }
            Self::VariableAlreadyExists(name) => {
                write!(formatter, "Environment variable {name} already exists.")
            }
            Self::VariableNotFound(name) => {
                write!(formatter, "Environment variable {name} was not found.")
            }
            Self::Store(error) => error.fmt(formatter),
            Self::Backup(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for EnvironmentServiceError {}

impl From<EnvironmentValidationError> for EnvironmentServiceError {
    fn from(error: EnvironmentValidationError) -> Self {
        Self::Validation(error)
    }
}

impl From<EnvironmentStoreError> for EnvironmentServiceError {
    fn from(error: EnvironmentStoreError) -> Self {
        Self::Store(error)
    }
}

impl From<BackupError> for EnvironmentServiceError {
    fn from(error: BackupError) -> Self {
        Self::Backup(error)
    }
}

pub struct EnvironmentService {
    store: Box<dyn EnvironmentStore>,
    backups: BackupStore,
}

impl EnvironmentService {
    pub fn new(store: Box<dyn EnvironmentStore>, backups: BackupStore) -> Self {
        Self { store, backups }
    }

    pub fn snapshot(&self) -> Result<EnvironmentSnapshot, EnvironmentServiceError> {
        Ok(EnvironmentSnapshot {
            user_variables: self.store.list(EnvironmentScope::User)?,
            system_variables: self.store.list(EnvironmentScope::System)?,
            is_elevated: self.store.is_elevated(),
            backups: self.backups.list()?,
            backup_directory: self.backups.directory().to_owned(),
        })
    }

    pub fn set_variable(
        &self,
        input: EnvironmentVariableInput,
    ) -> Result<EnvironmentSnapshot, EnvironmentServiceError> {
        input.validate()?;
        self.require_scope_permission(input.scope)?;
        let current = self.store.list(input.scope)?;

        if let Some(existing) = current
            .iter()
            .find(|variable| variable_names_equal(&variable.name, &input.name))
        {
            let is_same_variable = input
                .original_name
                .as_deref()
                .is_some_and(|name| variable_names_equal(name, &existing.name));
            if !is_same_variable {
                return Err(EnvironmentServiceError::VariableAlreadyExists(
                    existing.name.clone(),
                ));
            }
        }

        self.backups.create(input.scope, "beforeSet", current)?;
        self.store.set(&input)?;
        self.store.broadcast_change()?;
        self.snapshot()
    }

    pub fn delete_variable(
        &self,
        scope: EnvironmentScope,
        name: &str,
    ) -> Result<EnvironmentSnapshot, EnvironmentServiceError> {
        crate::domain::environment::validate_variable_name(name)?;
        self.require_scope_permission(scope)?;
        let current = self.store.list(scope)?;
        if !current
            .iter()
            .any(|variable| variable_names_equal(&variable.name, name))
        {
            return Err(EnvironmentServiceError::VariableNotFound(name.to_owned()));
        }

        self.backups.create(scope, "beforeDelete", current)?;
        self.store.delete(scope, name)?;
        self.store.broadcast_change()?;
        self.snapshot()
    }

    pub fn restore_backup(
        &self,
        backup_id: &str,
    ) -> Result<EnvironmentSnapshot, EnvironmentServiceError> {
        let backup = self.backups.load(backup_id)?;
        self.require_scope_permission(backup.scope)?;
        let current = self.store.list(backup.scope)?;
        self.backups
            .create(backup.scope, "beforeRestore", current.clone())?;

        let backup_names = backup
            .variables
            .iter()
            .map(|variable| variable.name.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        for variable in &current {
            if !backup_names.contains(&variable.name.to_ascii_lowercase()) {
                self.store.delete(backup.scope, &variable.name)?;
            }
        }

        for variable in backup.variables {
            self.store.set(&EnvironmentVariableInput {
                original_name: Some(variable.name.clone()),
                name: variable.name,
                value: variable.value,
                value_type: variable.value_type,
                scope: variable.scope,
            })?;
        }

        self.store.broadcast_change()?;
        self.snapshot()
    }

    pub fn analyze_path_entries(
        &self,
        entries: &[String],
    ) -> Result<Vec<PathEntryStatus>, EnvironmentServiceError> {
        let system = self.store.list(EnvironmentScope::System)?;
        let user = self.store.list(EnvironmentScope::User)?;
        let variables = system
            .into_iter()
            .chain(user)
            .map(|variable| (variable.name.to_ascii_lowercase(), variable.value))
            .collect::<HashMap<_, _>>();
        let duplicates = duplicate_path_entry_indexes(entries)
            .into_iter()
            .collect::<HashSet<_>>();

        Ok(entries
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let expanded_value = expand_percent_variables(value, &variables);
                let path = expanded_value.trim().trim_matches('"');
                let exists = !path.is_empty() && Path::new(path).exists();
                PathEntryStatus {
                    value: value.clone(),
                    expanded_value,
                    exists,
                    duplicate: duplicates.contains(&index),
                }
            })
            .collect())
    }

    fn require_scope_permission(
        &self,
        scope: EnvironmentScope,
    ) -> Result<(), EnvironmentServiceError> {
        if scope == EnvironmentScope::System && !self.store.is_elevated() {
            return Err(EnvironmentServiceError::ElevationRequired);
        }
        Ok(())
    }
}

fn expand_percent_variables(value: &str, variables: &HashMap<String, String>) -> String {
    let mut expanded = value.to_owned();
    for _ in 0..5 {
        let next = expand_percent_variables_once(&expanded, variables);
        if next == expanded {
            break;
        }
        expanded = next;
    }
    expanded
}

fn expand_percent_variables_once(value: &str, variables: &HashMap<String, String>) -> String {
    let bytes = value.as_bytes();
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;

    while let Some(relative_start) = bytes[cursor..].iter().position(|byte| *byte == b'%') {
        let start = cursor + relative_start;
        output.push_str(&value[cursor..start]);
        let Some(relative_end) = bytes[start + 1..].iter().position(|byte| *byte == b'%') else {
            output.push_str(&value[start..]);
            return output;
        };
        let end = start + 1 + relative_end;
        let name = &value[start + 1..end];
        if let Some(replacement) = variables.get(&name.to_ascii_lowercase()) {
            output.push_str(replacement);
        } else {
            output.push_str(&value[start..=end]);
        }
        cursor = end + 1;
    }

    output.push_str(&value[cursor..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::environment::EnvironmentValueType;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct MemoryState {
        variables: HashMap<EnvironmentScope, Vec<EnvironmentVariable>>,
        broadcasts: usize,
    }

    #[derive(Clone)]
    struct MemoryStore {
        state: Arc<Mutex<MemoryState>>,
        elevated: bool,
    }

    impl EnvironmentStore for MemoryStore {
        fn list(
            &self,
            scope: EnvironmentScope,
        ) -> Result<Vec<EnvironmentVariable>, EnvironmentStoreError> {
            Ok(self
                .state
                .lock()
                .unwrap()
                .variables
                .get(&scope)
                .cloned()
                .unwrap_or_default())
        }

        fn set(&self, input: &EnvironmentVariableInput) -> Result<(), EnvironmentStoreError> {
            let mut state = self.state.lock().unwrap();
            let variables = state.variables.entry(input.scope).or_default();
            if let Some(original_name) = &input.original_name {
                variables.retain(|variable| !variable_names_equal(&variable.name, original_name));
            }
            variables.retain(|variable| !variable_names_equal(&variable.name, &input.name));
            variables.push(EnvironmentVariable {
                name: input.name.clone(),
                value: input.value.clone(),
                value_type: input.value_type,
                scope: input.scope,
            });
            Ok(())
        }

        fn delete(&self, scope: EnvironmentScope, name: &str) -> Result<(), EnvironmentStoreError> {
            self.state
                .lock()
                .unwrap()
                .variables
                .entry(scope)
                .or_default()
                .retain(|variable| !variable_names_equal(&variable.name, name));
            Ok(())
        }

        fn is_elevated(&self) -> bool {
            self.elevated
        }

        fn broadcast_change(&self) -> Result<(), EnvironmentStoreError> {
            self.state.lock().unwrap().broadcasts += 1;
            Ok(())
        }
    }

    fn service(elevated: bool) -> (EnvironmentService, Arc<Mutex<MemoryState>>, PathBuf) {
        let state = Arc::new(Mutex::new(MemoryState::default()));
        let directory = std::env::temp_dir().join(format!(
            "env-manager-service-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let service = EnvironmentService::new(
            Box::new(MemoryStore {
                state: state.clone(),
                elevated,
            }),
            BackupStore::new(directory.clone()),
        );
        (service, state, directory)
    }

    fn input(scope: EnvironmentScope, name: &str, value: &str) -> EnvironmentVariableInput {
        EnvironmentVariableInput {
            original_name: None,
            name: name.to_owned(),
            value: value.to_owned(),
            value_type: EnvironmentValueType::String,
            scope,
        }
    }

    #[test]
    fn backs_up_and_sets_a_user_variable() {
        let (service, state, directory) = service(false);

        let snapshot = service
            .set_variable(input(EnvironmentScope::User, "JAVA_HOME", r"C:\Java"))
            .unwrap();

        assert_eq!(snapshot.user_variables.len(), 1);
        assert_eq!(snapshot.backups.len(), 1);
        assert_eq!(state.lock().unwrap().broadcasts, 1);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_system_mutation_before_creating_a_backup() {
        let (service, state, directory) = service(false);

        let result = service.set_variable(input(EnvironmentScope::System, "JAVA_HOME", "x"));

        assert!(matches!(
            result,
            Err(EnvironmentServiceError::ElevationRequired)
        ));
        assert_eq!(state.lock().unwrap().broadcasts, 0);
        assert!(!directory.exists());
    }

    #[test]
    fn restores_a_previous_scope_snapshot() {
        let (service, _state, directory) = service(true);
        let first = service
            .set_variable(input(EnvironmentScope::User, "JAVA_HOME", "first"))
            .unwrap();
        service
            .set_variable(EnvironmentVariableInput {
                original_name: Some("JAVA_HOME".to_owned()),
                ..input(EnvironmentScope::User, "JAVA_HOME", "second")
            })
            .unwrap();

        let restored = service.restore_backup(&first.backups[0].id).unwrap();

        assert!(restored.user_variables.is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn expands_registry_variables_for_path_diagnostics() {
        let (service, state, directory) = service(false);
        state.lock().unwrap().variables.insert(
            EnvironmentScope::User,
            vec![EnvironmentVariable {
                name: "TOOLS_HOME".to_owned(),
                value: r"C:\Tools".to_owned(),
                value_type: EnvironmentValueType::String,
                scope: EnvironmentScope::User,
            }],
        );

        let statuses = service
            .analyze_path_entries(&[r"%TOOLS_HOME%\bin".to_owned()])
            .unwrap();

        assert_eq!(statuses[0].expanded_value, r"C:\Tools\bin");
        if directory.exists() {
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
}
