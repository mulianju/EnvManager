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
    use crate::domain::environment::{
        EnvironmentValueType, TransferMode, TransferVariableInput,
    };
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct MemoryState {
        variables: HashMap<EnvironmentScope, Vec<EnvironmentVariable>>,
        broadcasts: usize,
        set_calls: usize,
        delete_calls: usize,
        fail_next_delete: Option<(EnvironmentScope, String)>,
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
            state.set_calls += 1;
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
            let mut state = self.state.lock().unwrap();
            state.delete_calls += 1;
            let should_fail = state
                .fail_next_delete
                .as_ref()
                .is_some_and(|(failure_scope, failure_name)| {
                    *failure_scope == scope && variable_names_equal(failure_name, name)
                });
            if should_fail {
                state.fail_next_delete = None;
                return Err(EnvironmentStoreError::OperationFailed(
                    "injected delete failure".to_owned(),
                ));
            }
            state
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

    struct TestHarness {
        service: EnvironmentService,
        state: Arc<Mutex<MemoryState>>,
        directory: PathBuf,
    }

    impl Drop for TestHarness {
        fn drop(&mut self) {
            if self.directory.exists() {
                std::fs::remove_dir_all(&self.directory).unwrap();
            }
        }
    }

    fn service(elevated: bool) -> TestHarness {
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
        TestHarness {
            service,
            state,
            directory,
        }
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

    fn variable(scope: EnvironmentScope, name: &str, value: &str) -> EnvironmentVariable {
        EnvironmentVariable {
            name: name.to_owned(),
            value: value.to_owned(),
            value_type: EnvironmentValueType::String,
            scope,
        }
    }

    fn seed(
        state: &Arc<Mutex<MemoryState>>,
        scope: EnvironmentScope,
        variables: Vec<EnvironmentVariable>,
    ) {
        state.lock().unwrap().variables.insert(scope, variables);
    }

    fn transfer(
        source_scope: EnvironmentScope,
        target_scope: EnvironmentScope,
        name: &str,
        mode: TransferMode,
        overwrite: bool,
    ) -> TransferVariableInput {
        TransferVariableInput {
            source_scope,
            target_scope,
            name: name.to_owned(),
            mode,
            overwrite,
        }
    }

    fn assert_variable(
        snapshot: &EnvironmentSnapshot,
        scope: EnvironmentScope,
        name: &str,
        value: &str,
    ) {
        let variables = match scope {
            EnvironmentScope::User => &snapshot.user_variables,
            EnvironmentScope::System => &snapshot.system_variables,
        };
        let actual = variables
            .iter()
            .find(|variable| variable_names_equal(&variable.name, name))
            .unwrap();
        assert_eq!(actual.value, value);
        assert_eq!(actual.scope, scope);
    }

    #[test]
    fn set_returns_its_pre_mutation_backup_and_updated_snapshot() {
        let harness = service(false);

        let result = harness
            .service
            .set_variable(input(EnvironmentScope::User, "JAVA_HOME", r"C:\Java"))
            .unwrap();

        assert_eq!(result.undo_backup_ids.len(), 1);
        assert_variable(
            &result.snapshot,
            EnvironmentScope::User,
            "java_home",
            r"C:\Java",
        );
        let backup = harness
            .service
            .backups
            .load(&result.undo_backup_ids[0])
            .unwrap();
        assert_eq!(backup.scope, EnvironmentScope::User);
        assert_eq!(backup.reason, "beforeSet");
        assert!(backup.variables.is_empty());
        assert_eq!(harness.state.lock().unwrap().broadcasts, 1);
    }

    #[test]
    fn delete_returns_its_pre_mutation_backup_and_updated_snapshot() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "Path", r"C:\Tools")],
        );

        let result = harness
            .service
            .delete_variable(EnvironmentScope::User, "PATH")
            .unwrap();

        assert_eq!(result.undo_backup_ids.len(), 1);
        assert!(result.snapshot.user_variables.is_empty());
        let backup = harness
            .service
            .backups
            .load(&result.undo_backup_ids[0])
            .unwrap();
        assert_eq!(backup.scope, EnvironmentScope::User);
        assert_eq!(backup.reason, "beforeDelete");
        assert_eq!(backup.variables, vec![variable(EnvironmentScope::User, "Path", r"C:\Tools")]);
    }

    #[test]
    fn restore_returns_a_new_rollback_backup_instead_of_the_selected_backup() {
        let harness = service(true);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "first")],
        );
        let changed = harness
            .service
            .set_variable(EnvironmentVariableInput {
                original_name: Some("java_home".to_owned()),
                ..input(EnvironmentScope::User, "JAVA_HOME", "second")
            })
            .unwrap();
        let selected_backup_id = changed.undo_backup_ids[0].clone();

        let restored = harness
            .service
            .restore_backup(&selected_backup_id)
            .unwrap();

        assert_eq!(restored.undo_backup_ids.len(), 1);
        assert_ne!(restored.undo_backup_ids[0], selected_backup_id);
        assert_variable(
            &restored.snapshot,
            EnvironmentScope::User,
            "JAVA_HOME",
            "first",
        );
        let rollback = harness
            .service
            .backups
            .load(&restored.undo_backup_ids[0])
            .unwrap();
        assert_eq!(rollback.scope, EnvironmentScope::User);
        assert_eq!(rollback.reason, "beforeRestore");
        assert_eq!(rollback.variables[0].value, "second");
    }

    #[test]
    fn undo_restores_one_scope_and_returns_a_redo_backup() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "first")],
        );
        let changed = harness
            .service
            .set_variable(EnvironmentVariableInput {
                original_name: Some("JAVA_HOME".to_owned()),
                ..input(EnvironmentScope::User, "JAVA_HOME", "second")
            })
            .unwrap();

        let undone = harness
            .service
            .undo_mutation(&changed.undo_backup_ids)
            .unwrap();

        assert_variable(
            &undone.snapshot,
            EnvironmentScope::User,
            "JAVA_HOME",
            "first",
        );
        assert_eq!(undone.undo_backup_ids.len(), 1);
        let redo = harness
            .service
            .backups
            .load(&undone.undo_backup_ids[0])
            .unwrap();
        assert_eq!(redo.scope, EnvironmentScope::User);
        assert_eq!(redo.reason, "beforeUndo");
        assert_eq!(redo.variables[0].value, "second");
    }

    #[test]
    fn copy_user_to_system_keeps_the_source_and_returns_the_target_backup() {
        let harness = service(true);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", r"C:\Java")],
        );

        let result = harness
            .service
            .transfer_variable(transfer(
                EnvironmentScope::User,
                EnvironmentScope::System,
                "java_home",
                TransferMode::Copy,
                false,
            ))
            .unwrap();

        assert_variable(
            &result.snapshot,
            EnvironmentScope::User,
            "JAVA_HOME",
            r"C:\Java",
        );
        assert_variable(
            &result.snapshot,
            EnvironmentScope::System,
            "JAVA_HOME",
            r"C:\Java",
        );
        assert_eq!(result.undo_backup_ids.len(), 1);
        let backup = harness
            .service
            .backups
            .load(&result.undo_backup_ids[0])
            .unwrap();
        assert_eq!(backup.scope, EnvironmentScope::System);
        assert_eq!(backup.reason, "beforeTransfer");
        assert_eq!(harness.state.lock().unwrap().broadcasts, 1);
    }

    #[test]
    fn move_user_to_system_returns_backups_for_both_scopes() {
        let harness = service(true);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", r"C:\Java")],
        );

        let result = harness
            .service
            .transfer_variable(transfer(
                EnvironmentScope::User,
                EnvironmentScope::System,
                "JAVA_HOME",
                TransferMode::Move,
                false,
            ))
            .unwrap();

        assert!(result.snapshot.user_variables.is_empty());
        assert_variable(
            &result.snapshot,
            EnvironmentScope::System,
            "java_home",
            r"C:\Java",
        );
        assert_eq!(result.undo_backup_ids.len(), 2);
        assert_ne!(result.undo_backup_ids[0], result.undo_backup_ids[1]);
        let backups = result
            .undo_backup_ids
            .iter()
            .map(|id| harness.service.backups.load(id).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            backups
                .iter()
                .map(|backup| backup.scope)
                .collect::<HashSet<_>>(),
            HashSet::from([EnvironmentScope::User, EnvironmentScope::System])
        );
        assert!(
            backups
                .iter()
                .all(|backup| backup.reason == "beforeTransfer")
        );
        assert_eq!(harness.state.lock().unwrap().broadcasts, 1);
    }

    #[test]
    fn transfer_rejects_a_case_insensitive_destination_conflict_without_side_effects() {
        let harness = service(true);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "Java_Home", "source")],
        );
        seed(
            &harness.state,
            EnvironmentScope::System,
            vec![variable(EnvironmentScope::System, "JAVA_HOME", "destination")],
        );

        let result = harness.service.transfer_variable(transfer(
            EnvironmentScope::User,
            EnvironmentScope::System,
            "java_home",
            TransferMode::Copy,
            false,
        ));

        assert!(matches!(
            result,
            Err(EnvironmentServiceError::VariableAlreadyExists(ref name)) if name == "JAVA_HOME"
        ));
        let state = harness.state.lock().unwrap();
        assert_eq!(state.set_calls, 0);
        assert_eq!(state.delete_calls, 0);
        assert_eq!(state.broadcasts, 0);
        assert!(!harness.directory.exists());
        assert_eq!(
            state.variables[&EnvironmentScope::System][0].value,
            "destination"
        );
    }

    #[test]
    fn transfer_overwrites_a_case_insensitive_destination_when_requested() {
        let harness = service(true);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "Java_Home", "source")],
        );
        seed(
            &harness.state,
            EnvironmentScope::System,
            vec![variable(EnvironmentScope::System, "JAVA_HOME", "destination")],
        );

        let result = harness
            .service
            .transfer_variable(transfer(
                EnvironmentScope::User,
                EnvironmentScope::System,
                "java_home",
                TransferMode::Copy,
                true,
            ))
            .unwrap();

        assert_variable(
            &result.snapshot,
            EnvironmentScope::System,
            "JAVA_HOME",
            "source",
        );
        assert_eq!(result.undo_backup_ids.len(), 1);
        let backup = harness
            .service
            .backups
            .load(&result.undo_backup_ids[0])
            .unwrap();
        assert_eq!(backup.variables[0].value, "destination");
    }

    #[test]
    fn transfer_checks_system_write_permission_before_backups_or_writes() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "source")],
        );

        let result = harness.service.transfer_variable(transfer(
            EnvironmentScope::User,
            EnvironmentScope::System,
            "JAVA_HOME",
            TransferMode::Copy,
            false,
        ));

        assert!(matches!(
            result,
            Err(EnvironmentServiceError::ElevationRequired)
        ));
        let state = harness.state.lock().unwrap();
        assert_eq!(state.set_calls, 0);
        assert_eq!(state.delete_calls, 0);
        assert_eq!(state.broadcasts, 0);
        assert!(!harness.directory.exists());
    }

    #[test]
    fn moving_from_system_requires_elevation_before_backups_or_writes() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::System,
            vec![variable(EnvironmentScope::System, "JAVA_HOME", "source")],
        );

        let result = harness.service.transfer_variable(transfer(
            EnvironmentScope::System,
            EnvironmentScope::User,
            "JAVA_HOME",
            TransferMode::Move,
            false,
        ));

        assert!(matches!(
            result,
            Err(EnvironmentServiceError::ElevationRequired)
        ));
        let state = harness.state.lock().unwrap();
        assert_eq!(state.set_calls, 0);
        assert_eq!(state.delete_calls, 0);
        assert_eq!(state.broadcasts, 0);
        assert!(!harness.directory.exists());
    }

    #[test]
    fn copying_from_system_to_user_only_reads_the_system_scope() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::System,
            vec![variable(EnvironmentScope::System, "JAVA_HOME", "source")],
        );

        let result = harness
            .service
            .transfer_variable(transfer(
                EnvironmentScope::System,
                EnvironmentScope::User,
                "java_home",
                TransferMode::Copy,
                false,
            ))
            .unwrap();

        assert_variable(
            &result.snapshot,
            EnvironmentScope::System,
            "JAVA_HOME",
            "source",
        );
        assert_variable(
            &result.snapshot,
            EnvironmentScope::User,
            "JAVA_HOME",
            "source",
        );
        assert_eq!(result.undo_backup_ids.len(), 1);
        let backup = harness
            .service
            .backups
            .load(&result.undo_backup_ids[0])
            .unwrap();
        assert_eq!(backup.scope, EnvironmentScope::User);
    }

    #[test]
    fn transfer_rejects_a_missing_source_without_side_effects() {
        let harness = service(true);

        let result = harness.service.transfer_variable(transfer(
            EnvironmentScope::User,
            EnvironmentScope::System,
            "MISSING",
            TransferMode::Copy,
            false,
        ));

        assert!(matches!(
            result,
            Err(EnvironmentServiceError::VariableNotFound(ref name)) if name == "MISSING"
        ));
        let state = harness.state.lock().unwrap();
        assert_eq!(state.set_calls, 0);
        assert_eq!(state.delete_calls, 0);
        assert_eq!(state.broadcasts, 0);
        assert!(!harness.directory.exists());
    }

    #[test]
    fn transfer_rejects_identical_source_and_target_scopes() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "source")],
        );

        let result = harness.service.transfer_variable(transfer(
            EnvironmentScope::User,
            EnvironmentScope::User,
            "JAVA_HOME",
            TransferMode::Copy,
            false,
        ));

        assert!(matches!(
            result,
            Err(EnvironmentServiceError::InvalidTransfer(_))
        ));
        let state = harness.state.lock().unwrap();
        assert_eq!(state.set_calls, 0);
        assert_eq!(state.delete_calls, 0);
        assert_eq!(state.broadcasts, 0);
        assert!(!harness.directory.exists());
    }

    #[test]
    fn failed_move_rolls_back_the_target_and_does_not_broadcast() {
        let harness = service(true);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "source")],
        );
        seed(
            &harness.state,
            EnvironmentScope::System,
            vec![variable(EnvironmentScope::System, "JAVA_HOME", "destination")],
        );
        harness.state.lock().unwrap().fail_next_delete = Some((
            EnvironmentScope::User,
            "java_home".to_owned(),
        ));

        let result = harness.service.transfer_variable(transfer(
            EnvironmentScope::User,
            EnvironmentScope::System,
            "JAVA_HOME",
            TransferMode::Move,
            true,
        ));

        assert!(matches!(result, Err(EnvironmentServiceError::Store(_))));
        let state = harness.state.lock().unwrap();
        assert_eq!(
            state.variables[&EnvironmentScope::User],
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "source")]
        );
        assert_eq!(
            state.variables[&EnvironmentScope::System],
            vec![variable(
                EnvironmentScope::System,
                "JAVA_HOME",
                "destination"
            )]
        );
        assert_eq!(state.set_calls, 2);
        assert_eq!(state.delete_calls, 1);
        assert_eq!(state.broadcasts, 0);
    }

    #[test]
    fn undo_restores_user_and_system_scopes_together() {
        let harness = service(true);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "USER_VALUE", "before-user")],
        );
        seed(
            &harness.state,
            EnvironmentScope::System,
            vec![variable(
                EnvironmentScope::System,
                "SYSTEM_VALUE",
                "before-system",
            )],
        );
        let user_change = harness
            .service
            .set_variable(EnvironmentVariableInput {
                original_name: Some("USER_VALUE".to_owned()),
                ..input(EnvironmentScope::User, "USER_VALUE", "after-user")
            })
            .unwrap();
        let system_change = harness
            .service
            .set_variable(EnvironmentVariableInput {
                original_name: Some("SYSTEM_VALUE".to_owned()),
                ..input(EnvironmentScope::System, "SYSTEM_VALUE", "after-system")
            })
            .unwrap();
        let undo_ids = vec![
            user_change.undo_backup_ids[0].clone(),
            system_change.undo_backup_ids[0].clone(),
        ];

        let undone = harness.service.undo_mutation(&undo_ids).unwrap();

        assert_variable(
            &undone.snapshot,
            EnvironmentScope::User,
            "USER_VALUE",
            "before-user",
        );
        assert_variable(
            &undone.snapshot,
            EnvironmentScope::System,
            "SYSTEM_VALUE",
            "before-system",
        );
        assert_eq!(undone.undo_backup_ids.len(), 2);
        let redo_scopes = undone
            .undo_backup_ids
            .iter()
            .map(|id| harness.service.backups.load(id).unwrap().scope)
            .collect::<HashSet<_>>();
        assert_eq!(
            redo_scopes,
            HashSet::from([EnvironmentScope::User, EnvironmentScope::System])
        );
    }

    #[test]
    fn undo_rejects_multiple_backups_for_the_same_scope() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "initial")],
        );
        let first = harness
            .service
            .set_variable(EnvironmentVariableInput {
                original_name: Some("JAVA_HOME".to_owned()),
                ..input(EnvironmentScope::User, "JAVA_HOME", "first")
            })
            .unwrap();
        let second = harness
            .service
            .set_variable(EnvironmentVariableInput {
                original_name: Some("JAVA_HOME".to_owned()),
                ..input(EnvironmentScope::User, "JAVA_HOME", "second")
            })
            .unwrap();
        let broadcasts_before_undo = harness.state.lock().unwrap().broadcasts;
        let backups_before_undo = harness.service.snapshot().unwrap().backups.len();

        let result = harness.service.undo_mutation(&[
            first.undo_backup_ids[0].clone(),
            second.undo_backup_ids[0].clone(),
        ]);

        assert!(result.is_err());
        let state = harness.state.lock().unwrap();
        assert_eq!(state.variables[&EnvironmentScope::User][0].value, "second");
        assert_eq!(state.broadcasts, broadcasts_before_undo);
        drop(state);
        assert_eq!(
            harness.service.snapshot().unwrap().backups.len(),
            backups_before_undo
        );
    }

    #[test]
    fn expands_registry_variables_for_path_diagnostics() {
        let harness = service(false);
        harness.state.lock().unwrap().variables.insert(
            EnvironmentScope::User,
            vec![EnvironmentVariable {
                name: "TOOLS_HOME".to_owned(),
                value: r"C:\Tools".to_owned(),
                value_type: EnvironmentValueType::String,
                scope: EnvironmentScope::User,
            }],
        );

        let statuses = harness
            .service
            .analyze_path_entries(&[r"%TOOLS_HOME%\bin".to_owned()])
            .unwrap();

        assert_eq!(statuses[0].expanded_value, r"C:\Tools\bin");
    }
}
