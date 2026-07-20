use crate::domain::environment::{
    EnvironmentScope, EnvironmentValidationError, EnvironmentValueType, EnvironmentVariable,
    EnvironmentVariableInput, TransferMode, TransferVariableInput, duplicate_path_entry_indexes,
    is_path_variable, join_path_entries, parse_path_entries, variable_names_equal,
};
use crate::platform::{EnvironmentStore, EnvironmentStoreError};
use crate::services::backup::{BackupDocument, BackupError, BackupStore, BackupSummary};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub user_variables: Vec<EnvironmentVariable>,
    pub system_variables: Vec<EnvironmentVariable>,
    pub effective_variables: Vec<EffectiveEnvironmentVariable>,
    pub revision: String,
    pub is_elevated: bool,
    pub backups: Vec<BackupSummary>,
    pub backup_directory: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectiveVariableSource {
    User,
    System,
    Combined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveEnvironmentVariable {
    pub name: String,
    pub value: String,
    pub value_type: EnvironmentValueType,
    pub source: EffectiveVariableSource,
    pub shadowed: bool,
    pub conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    pub snapshot: EnvironmentSnapshot,
    pub undo_backup_ids: Vec<String>,
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
    InvalidTransfer(String),
    UndoInvalid(String),
    TransactionRollbackFailed(String),
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
            Self::InvalidTransfer(message) => write!(formatter, "Invalid transfer: {message}"),
            Self::UndoInvalid(message) => write!(formatter, "Invalid undo request: {message}"),
            Self::TransactionRollbackFailed(message) => {
                write!(
                    formatter,
                    "Environment transaction rollback failed: {message}"
                )
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
        let user_variables = self.store.list(EnvironmentScope::User)?;
        let system_variables = self.store.list(EnvironmentScope::System)?;
        let effective_variables = build_effective_variables(&user_variables, &system_variables);
        let revision = environment_revision(&user_variables, &system_variables);
        Ok(EnvironmentSnapshot {
            user_variables,
            system_variables,
            effective_variables,
            revision,
            is_elevated: self.store.is_elevated(),
            backups: self.backups.list()?,
            backup_directory: self.backups.directory().to_owned(),
        })
    }

    pub fn revision(&self) -> Result<String, EnvironmentServiceError> {
        let user_variables = self.store.list(EnvironmentScope::User)?;
        let system_variables = self.store.list(EnvironmentScope::System)?;
        Ok(environment_revision(&user_variables, &system_variables))
    }

    pub fn launch_powershell(&self) -> Result<(), EnvironmentServiceError> {
        let user_variables = self.store.list(EnvironmentScope::User)?;
        let system_variables = self.store.list(EnvironmentScope::System)?;
        let base = std::env::vars().collect::<Vec<_>>();
        let environment = compose_process_environment(&base, &user_variables, &system_variables);
        crate::platform::launch_powershell(&environment)?;
        Ok(())
    }

    pub fn set_variable(
        &self,
        input: EnvironmentVariableInput,
    ) -> Result<MutationResult, EnvironmentServiceError> {
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

        let backup = self
            .backups
            .create(input.scope, "beforeSet", current.clone())?;
        self.store.set(&input)?;
        self.finalize_transaction(vec![backup.id], &[(input.scope, current)])
    }

    pub fn delete_variable(
        &self,
        scope: EnvironmentScope,
        name: &str,
    ) -> Result<MutationResult, EnvironmentServiceError> {
        crate::domain::environment::validate_variable_name(name)?;
        self.require_scope_permission(scope)?;
        let current = self.store.list(scope)?;
        if !current
            .iter()
            .any(|variable| variable_names_equal(&variable.name, name))
        {
            return Err(EnvironmentServiceError::VariableNotFound(name.to_owned()));
        }

        let backup = self
            .backups
            .create(scope, "beforeDelete", current.clone())?;
        self.store.delete(scope, name)?;
        self.finalize_transaction(vec![backup.id], &[(scope, current)])
    }

    pub fn restore_backup(
        &self,
        backup_id: &str,
    ) -> Result<MutationResult, EnvironmentServiceError> {
        let backup = self.backups.load(backup_id)?;
        self.require_scope_permission(backup.scope)?;
        let current = self.store.list(backup.scope)?;
        let rollback = self
            .backups
            .create(backup.scope, "beforeRestore", current.clone())?;

        if let Err(error) = self.restore_document(&backup) {
            return Err(self.rollback_error(error, &[(backup.scope, current)]));
        }

        self.finalize_transaction(vec![rollback.id], &[(backup.scope, current)])
    }

    pub fn undo_mutation(
        &self,
        backup_ids: &[String],
    ) -> Result<MutationResult, EnvironmentServiceError> {
        if backup_ids.is_empty() {
            return Err(EnvironmentServiceError::UndoInvalid(
                "At least one backup is required.".to_owned(),
            ));
        }

        let backups = backup_ids
            .iter()
            .map(|id| self.backups.load(id))
            .collect::<Result<Vec<_>, _>>()?;
        let mut scopes = HashSet::new();
        if let Some(duplicate) = backups
            .iter()
            .map(|backup| backup.scope)
            .find(|scope| !scopes.insert(*scope))
        {
            return Err(EnvironmentServiceError::UndoInvalid(format!(
                "Only one backup can be restored for the {duplicate:?} scope."
            )));
        }
        for backup in &backups {
            self.require_scope_permission(backup.scope)?;
        }

        let mut rollback_states = Vec::with_capacity(backups.len());
        let mut rollback_ids = Vec::with_capacity(backups.len());
        for backup in &backups {
            let variables = self.store.list(backup.scope)?;
            let summary = self
                .backups
                .create(backup.scope, "beforeUndo", variables.clone())?;
            rollback_ids.push(summary.id);
            rollback_states.push((backup.scope, variables));
        }

        for (index, backup) in backups.iter().enumerate() {
            if let Err(error) = self.restore_document(backup) {
                return Err(self.rollback_error(error, &rollback_states[..=index]));
            }
        }

        self.finalize_transaction(rollback_ids, &rollback_states)
    }

    pub fn transfer_variable(
        &self,
        input: TransferVariableInput,
    ) -> Result<MutationResult, EnvironmentServiceError> {
        input.validate()?;
        if input.source_scope == input.target_scope {
            return Err(EnvironmentServiceError::InvalidTransfer(
                "Source and target scopes must be different.".to_owned(),
            ));
        }

        self.require_scope_permission(input.target_scope)?;
        if input.mode == TransferMode::Move {
            self.require_scope_permission(input.source_scope)?;
        }

        let source_variables = self.store.list(input.source_scope)?;
        let target_variables = self.store.list(input.target_scope)?;
        let source = source_variables
            .iter()
            .find(|variable| variable_names_equal(&variable.name, &input.name))
            .ok_or_else(|| EnvironmentServiceError::VariableNotFound(input.name.clone()))?;
        let destination = target_variables
            .iter()
            .find(|variable| variable_names_equal(&variable.name, &input.name));
        if let Some(destination) = destination {
            if !input.overwrite {
                return Err(EnvironmentServiceError::VariableAlreadyExists(
                    destination.name.clone(),
                ));
            }
        }

        let target_backup = self.backups.create(
            input.target_scope,
            "beforeTransfer",
            target_variables.clone(),
        )?;
        let mut undo_backup_ids = vec![target_backup.id];
        if input.mode == TransferMode::Move {
            let source_backup = self.backups.create(
                input.source_scope,
                "beforeTransfer",
                source_variables.clone(),
            )?;
            undo_backup_ids.push(source_backup.id);
        }

        let target_input = EnvironmentVariableInput {
            original_name: destination.map(|variable| variable.name.clone()),
            name: source.name.clone(),
            value: source.value.clone(),
            value_type: source.value_type,
            scope: input.target_scope,
        };
        if let Err(error) = self.store.set(&target_input) {
            return Err(
                self.rollback_error(error.into(), &[(input.target_scope, target_variables)])
            );
        }

        if input.mode == TransferMode::Move {
            if let Err(error) = self.store.delete(input.source_scope, &source.name) {
                return Err(
                    self.rollback_error(error.into(), &[(input.target_scope, target_variables)])
                );
            }
        }

        let mut rollback_states = vec![(input.target_scope, target_variables)];
        if input.mode == TransferMode::Move {
            rollback_states.push((input.source_scope, source_variables));
        }
        self.finalize_transaction(undo_backup_ids, &rollback_states)
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

    fn finalize_transaction(
        &self,
        undo_backup_ids: Vec<String>,
        rollback_states: &[(EnvironmentScope, Vec<EnvironmentVariable>)],
    ) -> Result<MutationResult, EnvironmentServiceError> {
        let result = self
            .store
            .broadcast_change()
            .map_err(EnvironmentServiceError::from)
            .and_then(|()| self.snapshot());
        match result {
            Ok(snapshot) => Ok(MutationResult {
                snapshot,
                undo_backup_ids,
            }),
            Err(error) => {
                let error = self.rollback_error(error, rollback_states);
                let _ = self.store.broadcast_change();
                Err(error)
            }
        }
    }

    fn restore_document(&self, document: &BackupDocument) -> Result<(), EnvironmentServiceError> {
        self.restore_scope(document.scope, &document.variables)
    }

    fn restore_scope(
        &self,
        scope: EnvironmentScope,
        variables: &[EnvironmentVariable],
    ) -> Result<(), EnvironmentServiceError> {
        let current = self.store.list(scope)?;
        let target_names = variables
            .iter()
            .map(|variable| variable.name.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        for variable in &current {
            if !target_names.contains(&variable.name.to_ascii_lowercase()) {
                self.store.delete(scope, &variable.name)?;
            }
        }

        for variable in variables {
            let original_name = current
                .iter()
                .find(|current| variable_names_equal(&current.name, &variable.name))
                .map(|current| current.name.clone());
            self.store.set(&EnvironmentVariableInput {
                original_name,
                name: variable.name.clone(),
                value: variable.value.clone(),
                value_type: variable.value_type,
                scope,
            })?;
        }
        Ok(())
    }

    fn rollback_error(
        &self,
        error: EnvironmentServiceError,
        rollback_states: &[(EnvironmentScope, Vec<EnvironmentVariable>)],
    ) -> EnvironmentServiceError {
        let mut rollback_error = None;
        for (scope, variables) in rollback_states.iter().rev() {
            if let Err(error) = self.restore_scope(*scope, variables) {
                rollback_error.get_or_insert(error);
            }
        }
        match rollback_error {
            Some(rollback_error) => EnvironmentServiceError::TransactionRollbackFailed(format!(
                "{error}; rollback error: {rollback_error}"
            )),
            None => error,
        }
    }
}

pub fn build_effective_variables(
    user: &[EnvironmentVariable],
    system: &[EnvironmentVariable],
) -> Vec<EffectiveEnvironmentVariable> {
    let user_by_name = user
        .iter()
        .map(|variable| (variable.name.to_ascii_lowercase(), variable))
        .collect::<HashMap<_, _>>();
    let system_by_name = system
        .iter()
        .map(|variable| (variable.name.to_ascii_lowercase(), variable))
        .collect::<HashMap<_, _>>();
    let names = user_by_name
        .keys()
        .chain(system_by_name.keys())
        .cloned()
        .collect::<HashSet<_>>();

    let mut effective = names
        .into_iter()
        .filter_map(|name| {
            let user_variable = user_by_name.get(&name).copied();
            let system_variable = system_by_name.get(&name).copied();

            if is_path_variable(&name) {
                return match (user_variable, system_variable) {
                    (Some(user_path), Some(system_path)) => {
                        let entries = parse_path_entries(&system_path.value)
                            .into_iter()
                            .chain(parse_path_entries(&user_path.value))
                            .collect::<Vec<_>>();
                        Some(EffectiveEnvironmentVariable {
                            name: user_path.name.clone(),
                            value: join_path_entries(&entries),
                            value_type: if user_path.value_type
                                == EnvironmentValueType::ExpandableString
                                || system_path.value_type == EnvironmentValueType::ExpandableString
                            {
                                EnvironmentValueType::ExpandableString
                            } else {
                                EnvironmentValueType::String
                            },
                            source: EffectiveVariableSource::Combined,
                            shadowed: false,
                            conflict: false,
                        })
                    }
                    (Some(variable), None) => Some(effective_from_single(
                        variable,
                        EffectiveVariableSource::User,
                    )),
                    (None, Some(variable)) => Some(effective_from_single(
                        variable,
                        EffectiveVariableSource::System,
                    )),
                    (None, None) => None,
                };
            }

            match (user_variable, system_variable) {
                (Some(user_variable), Some(system_variable)) => {
                    Some(EffectiveEnvironmentVariable {
                        name: user_variable.name.clone(),
                        value: user_variable.value.clone(),
                        value_type: user_variable.value_type,
                        source: EffectiveVariableSource::User,
                        shadowed: true,
                        conflict: user_variable.value != system_variable.value
                            || user_variable.value_type != system_variable.value_type,
                    })
                }
                (Some(variable), None) => Some(effective_from_single(
                    variable,
                    EffectiveVariableSource::User,
                )),
                (None, Some(variable)) => Some(effective_from_single(
                    variable,
                    EffectiveVariableSource::System,
                )),
                (None, None) => None,
            }
        })
        .collect::<Vec<_>>();
    effective.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    effective
}

fn effective_from_single(
    variable: &EnvironmentVariable,
    source: EffectiveVariableSource,
) -> EffectiveEnvironmentVariable {
    EffectiveEnvironmentVariable {
        name: variable.name.clone(),
        value: variable.value.clone(),
        value_type: variable.value_type,
        source,
        shadowed: false,
        conflict: false,
    }
}

pub fn environment_revision(
    user: &[EnvironmentVariable],
    system: &[EnvironmentVariable],
) -> String {
    let mut records = user
        .iter()
        .map(|variable| (EnvironmentScope::User, variable))
        .chain(
            system
                .iter()
                .map(|variable| (EnvironmentScope::System, variable)),
        )
        .collect::<Vec<_>>();
    records.sort_by(|(left_scope, left), (right_scope, right)| {
        scope_order(*left_scope)
            .cmp(&scope_order(*right_scope))
            .then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| {
                value_type_order(left.value_type).cmp(&value_type_order(right.value_type))
            })
            .then_with(|| left.value.cmp(&right.value))
    });

    let mut hash = 0xcbf29ce484222325u64;
    for (scope, variable) in records {
        hash_revision_field(&mut hash, &[scope_order(scope)]);
        hash_revision_field(&mut hash, variable.name.to_ascii_lowercase().as_bytes());
        hash_revision_field(&mut hash, variable.name.as_bytes());
        hash_revision_field(&mut hash, &[value_type_order(variable.value_type)]);
        hash_revision_field(&mut hash, variable.value.as_bytes());
    }
    format!("{hash:016x}")
}

fn scope_order(scope: EnvironmentScope) -> u8 {
    match scope {
        EnvironmentScope::User => 0,
        EnvironmentScope::System => 1,
    }
}

fn value_type_order(value_type: EnvironmentValueType) -> u8 {
    match value_type {
        EnvironmentValueType::String => 0,
        EnvironmentValueType::ExpandableString => 1,
    }
}

fn hash_revision_field(hash: &mut u64, value: &[u8]) {
    for byte in (value.len() as u64).to_le_bytes().iter().chain(value) {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

pub fn compose_process_environment(
    base: &[(String, String)],
    user: &[EnvironmentVariable],
    system: &[EnvironmentVariable],
) -> Vec<(String, String)> {
    let mut composed = HashMap::<String, (String, String)>::new();
    for (name, value) in base.iter().filter(|(name, _)| !is_path_variable(name)) {
        composed.insert(name.to_ascii_lowercase(), (name.clone(), value.clone()));
    }
    for variable in system
        .iter()
        .filter(|variable| !is_path_variable(&variable.name))
    {
        composed.insert(
            variable.name.to_ascii_lowercase(),
            (variable.name.clone(), variable.value.clone()),
        );
    }
    for variable in user
        .iter()
        .filter(|variable| !is_path_variable(&variable.name))
    {
        composed.insert(
            variable.name.to_ascii_lowercase(),
            (variable.name.clone(), variable.value.clone()),
        );
    }

    let system_path = system
        .iter()
        .find(|variable| is_path_variable(&variable.name));
    let user_path = user
        .iter()
        .find(|variable| is_path_variable(&variable.name));
    if system_path.is_some() || user_path.is_some() {
        let entries = system_path
            .into_iter()
            .flat_map(|variable| parse_path_entries(&variable.value))
            .chain(
                user_path
                    .into_iter()
                    .flat_map(|variable| parse_path_entries(&variable.value)),
            )
            .collect::<Vec<_>>();
        let display_name = user_path
            .or(system_path)
            .expect("registry path was checked")
            .name
            .clone();
        composed.insert(
            "path".to_owned(),
            (display_name, join_path_entries(&entries)),
        );
    }

    let raw_values = composed
        .iter()
        .map(|(normalized_name, (_, value))| (normalized_name.clone(), value.clone()))
        .collect::<HashMap<_, _>>();
    let mut result = composed
        .into_values()
        .map(|(name, value)| {
            let value = expand_percent_variables(&value, &raw_values);
            (name, value)
        })
        .collect::<Vec<_>>();
    result.sort_by(|(left, _), (right, _)| {
        left.to_ascii_lowercase()
            .cmp(&right.to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
    result
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
    use crate::domain::environment::{EnvironmentValueType, TransferMode, TransferVariableInput};
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
        fail_next_broadcast: bool,
        fail_list_after_next_mutation: bool,
        fail_next_list: bool,
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
            let mut state = self.state.lock().unwrap();
            if state.fail_next_list {
                state.fail_next_list = false;
                return Err(EnvironmentStoreError::OperationFailed(
                    "injected list failure".to_owned(),
                ));
            }
            Ok(state.variables.get(&scope).cloned().unwrap_or_default())
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
            if state.fail_list_after_next_mutation {
                state.fail_list_after_next_mutation = false;
                state.fail_next_list = true;
            }
            Ok(())
        }

        fn delete(&self, scope: EnvironmentScope, name: &str) -> Result<(), EnvironmentStoreError> {
            let mut state = self.state.lock().unwrap();
            state.delete_calls += 1;
            let should_fail =
                state
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
            if state.fail_list_after_next_mutation {
                state.fail_list_after_next_mutation = false;
                state.fail_next_list = true;
            }
            Ok(())
        }

        fn is_elevated(&self) -> bool {
            self.elevated
        }

        fn broadcast_change(&self) -> Result<(), EnvironmentStoreError> {
            let mut state = self.state.lock().unwrap();
            if state.fail_next_broadcast {
                state.fail_next_broadcast = false;
                return Err(EnvironmentStoreError::OperationFailed(
                    "injected broadcast failure".to_owned(),
                ));
            }
            state.broadcasts += 1;
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

    fn expandable_variable(
        scope: EnvironmentScope,
        name: &str,
        value: &str,
    ) -> EnvironmentVariable {
        EnvironmentVariable {
            name: name.to_owned(),
            value: value.to_owned(),
            value_type: EnvironmentValueType::ExpandableString,
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
    fn effective_variables_use_case_insensitive_user_precedence_and_scope_metadata() {
        let user = vec![
            variable(EnvironmentScope::User, "Java_Home", r"C:\UserJava"),
            variable(EnvironmentScope::User, "USER_ONLY", "user"),
        ];
        let system = vec![
            expandable_variable(EnvironmentScope::System, "JAVA_HOME", r"C:\SystemJava"),
            variable(EnvironmentScope::System, "SYSTEM_ONLY", "system"),
        ];

        let effective = build_effective_variables(&user, &system);
        let java_home = effective
            .iter()
            .find(|variable| variable_names_equal(&variable.name, "java_home"))
            .unwrap();
        assert_eq!(java_home.name, "Java_Home");
        assert_eq!(java_home.value, r"C:\UserJava");
        assert_eq!(java_home.value_type, EnvironmentValueType::String);
        assert_eq!(java_home.source, EffectiveVariableSource::User);
        assert!(java_home.shadowed);
        assert!(java_home.conflict);

        let user_only = effective
            .iter()
            .find(|variable| variable.name == "USER_ONLY")
            .unwrap();
        assert_eq!(user_only.source, EffectiveVariableSource::User);
        assert!(!user_only.shadowed);
        assert!(!user_only.conflict);

        let system_only = effective
            .iter()
            .find(|variable| variable.name == "SYSTEM_ONLY")
            .unwrap();
        assert_eq!(system_only.source, EffectiveVariableSource::System);
        assert!(!system_only.shadowed);
        assert!(!system_only.conflict);
    }

    #[test]
    fn effective_path_combines_system_then_user_and_preserves_expandable_type() {
        let user = vec![expandable_variable(
            EnvironmentScope::User,
            "PATH",
            r"%USER_TOOLS%;C:\UserBin",
        )];
        let system = vec![variable(
            EnvironmentScope::System,
            "Path",
            r"C:\Windows;C:\SystemBin",
        )];

        let effective = build_effective_variables(&user, &system);

        assert_eq!(effective.len(), 1);
        assert_eq!(effective[0].name, "PATH");
        assert_eq!(
            effective[0].value,
            r"C:\Windows;C:\SystemBin;%USER_TOOLS%;C:\UserBin"
        );
        assert_eq!(
            effective[0].value_type,
            EnvironmentValueType::ExpandableString
        );
        assert_eq!(effective[0].source, EffectiveVariableSource::Combined);
        assert!(!effective[0].shadowed);
        assert!(!effective[0].conflict);
    }

    #[test]
    fn effective_path_keeps_its_single_scope_source() {
        let user_path = build_effective_variables(
            &[variable(EnvironmentScope::User, "Path", r"C:\UserBin")],
            &[],
        );
        let system_path = build_effective_variables(
            &[],
            &[variable(EnvironmentScope::System, "PATH", r"C:\SystemBin")],
        );

        assert_eq!(user_path[0].source, EffectiveVariableSource::User);
        assert_eq!(user_path[0].value, r"C:\UserBin");
        assert_eq!(system_path[0].source, EffectiveVariableSource::System);
        assert_eq!(system_path[0].value, r"C:\SystemBin");
    }

    #[test]
    fn effective_variables_are_stably_sorted_and_keep_the_winning_display_name() {
        let user = vec![
            variable(EnvironmentScope::User, "beta", "user-beta"),
            variable(EnvironmentScope::User, "Alpha_Name", "user-alpha"),
        ];
        let system = vec![
            variable(EnvironmentScope::System, "ZED", "system-zed"),
            variable(EnvironmentScope::System, "ALPHA_NAME", "system-alpha"),
        ];

        let effective = build_effective_variables(&user, &system);
        let names = effective
            .iter()
            .map(|variable| variable.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["Alpha_Name", "beta", "ZED"]);
    }

    #[test]
    fn environment_revision_is_order_independent_and_tracks_all_registry_data() {
        let user = vec![
            variable(EnvironmentScope::User, "ALPHA", "one"),
            variable(EnvironmentScope::User, "BETA", "two"),
        ];
        let system = vec![
            variable(EnvironmentScope::System, "GAMMA", "three"),
            variable(EnvironmentScope::System, "DELTA", "four"),
        ];
        let baseline = environment_revision(&user, &system);

        let mut reordered_user = user.clone();
        reordered_user.reverse();
        let mut reordered_system = system.clone();
        reordered_system.reverse();
        assert_eq!(
            baseline,
            environment_revision(&reordered_user, &reordered_system)
        );
        assert_eq!(baseline, environment_revision(&user, &system));

        let mut changed_value = user.clone();
        changed_value[0].value = "changed".to_owned();
        assert_ne!(baseline, environment_revision(&changed_value, &system));

        let mut changed_type = user.clone();
        changed_type[0].value_type = EnvironmentValueType::ExpandableString;
        assert_ne!(baseline, environment_revision(&changed_type, &system));

        let mut changed_display_name = user.clone();
        changed_display_name[0].name = "alpha".to_owned();
        assert_ne!(
            baseline,
            environment_revision(&changed_display_name, &system)
        );

        let moved_to_system = vec![
            system[0].clone(),
            system[1].clone(),
            variable(EnvironmentScope::System, "ALPHA", "one"),
        ];
        assert_ne!(baseline, environment_revision(&user[1..], &moved_to_system));
        assert_ne!(baseline, environment_revision(&system, &user));
    }

    #[test]
    fn snapshot_and_revision_share_the_same_raw_registry_projection() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", r"C:\Java")],
        );
        seed(
            &harness.state,
            EnvironmentScope::System,
            vec![variable(EnvironmentScope::System, "Path", r"C:\Windows")],
        );

        let snapshot = harness.service.snapshot().unwrap();

        assert_eq!(
            snapshot.effective_variables,
            build_effective_variables(&snapshot.user_variables, &snapshot.system_variables)
        );
        assert_eq!(
            snapshot.revision,
            environment_revision(&snapshot.user_variables, &snapshot.system_variables)
        );
        assert_eq!(harness.service.revision().unwrap(), snapshot.revision);
    }

    #[test]
    fn revision_does_not_read_or_parse_the_backup_list() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "TOOLS_HOME", r"C:\Tools")],
        );
        std::fs::create_dir_all(&harness.directory).unwrap();
        std::fs::write(harness.directory.join("invalid.json"), b"not json").unwrap();

        let revision = harness.service.revision().unwrap();

        assert_eq!(
            revision,
            environment_revision(
                &[variable(EnvironmentScope::User, "TOOLS_HOME", r"C:\Tools")],
                &[]
            )
        );
    }

    #[test]
    fn process_environment_composes_precedence_path_and_recursive_expansion() {
        let base = vec![
            ("DYNAMIC_BASE".to_owned(), "%Root%\\dynamic".to_owned()),
            ("ROOT".to_owned(), r"C:\Base".to_owned()),
            ("Path".to_owned(), r"C:\StaleBase".to_owned()),
            ("UNKNOWN_REF".to_owned(), "%MISSING%\\bin".to_owned()),
        ];
        let system = vec![
            variable(EnvironmentScope::System, "Root", r"C:\System"),
            expandable_variable(EnvironmentScope::System, "PATH", r"%ROOT%\SystemBin"),
            expandable_variable(EnvironmentScope::System, "CHAIN", "%ROOT%\\chain"),
        ];
        let user = vec![
            variable(EnvironmentScope::User, "rOoT", r"C:\User"),
            expandable_variable(EnvironmentScope::User, "Path", r"%CHAIN%\UserBin"),
        ];

        let composed = compose_process_environment(&base, &user, &system);
        let values = composed
            .iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value.as_str()))
            .collect::<HashMap<_, _>>();

        assert_eq!(values["root"], r"C:\User");
        assert_eq!(values["dynamic_base"], r"C:\User\dynamic");
        assert_eq!(values["chain"], r"C:\User\chain");
        assert_eq!(values["path"], r"C:\User\SystemBin;C:\User\chain\UserBin");
        assert!(!values["path"].contains("StaleBase"));
        assert_eq!(values["unknown_ref"], r"%MISSING%\bin");
    }

    #[test]
    fn process_environment_is_case_unique_sorted_and_keeps_user_display_names() {
        let base = vec![
            ("zeta".to_owned(), "base-zeta".to_owned()),
            ("TOOL_HOME".to_owned(), "base-tool".to_owned()),
        ];
        let system = vec![variable(
            EnvironmentScope::System,
            "Tool_Home",
            "system-tool",
        )];
        let user = vec![variable(EnvironmentScope::User, "tool_home", "user-tool")];

        let composed = compose_process_environment(&base, &user, &system);
        let normalized_names = composed
            .iter()
            .map(|(name, _)| name.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let unique_names = normalized_names.iter().collect::<HashSet<_>>();

        assert_eq!(unique_names.len(), composed.len());
        assert_eq!(normalized_names, vec!["tool_home", "zeta"]);
        assert_eq!(
            composed[0],
            ("tool_home".to_owned(), "user-tool".to_owned())
        );
    }

    #[test]
    fn process_environment_drops_stale_base_path_without_registry_path() {
        let base = vec![
            ("Path".to_owned(), r"C:\StaleBase".to_owned()),
            ("KEEP".to_owned(), "value".to_owned()),
        ];

        let composed = compose_process_environment(&base, &[], &[]);

        assert!(composed.iter().all(|(name, _)| !is_path_variable(name)));
        assert!(
            composed
                .iter()
                .any(|(name, value)| { variable_names_equal(name, "KEEP") && value == "value" })
        );
    }

    #[test]
    fn process_environment_bounds_cyclic_percent_expansion() {
        let user = vec![
            expandable_variable(EnvironmentScope::User, "A", "%B%"),
            expandable_variable(EnvironmentScope::User, "B", "%A%"),
        ];
        let started_at = std::time::Instant::now();

        let composed = compose_process_environment(&[], &user, &[]);

        assert!(started_at.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(composed.len(), 2);
        assert!(composed.iter().all(|(_, value)| value.contains('%')));
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
        assert_eq!(
            backup.variables,
            vec![variable(EnvironmentScope::User, "Path", r"C:\Tools")]
        );
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

        let restored = harness.service.restore_backup(&selected_backup_id).unwrap();

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
            vec![variable(
                EnvironmentScope::System,
                "JAVA_HOME",
                "destination",
            )],
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
            vec![variable(
                EnvironmentScope::System,
                "JAVA_HOME",
                "destination",
            )],
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
            vec![variable(
                EnvironmentScope::System,
                "JAVA_HOME",
                "destination",
            )],
        );
        harness.state.lock().unwrap().fail_next_delete =
            Some((EnvironmentScope::User, "java_home".to_owned()));

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
            vec![variable(
                EnvironmentScope::User,
                "USER_VALUE",
                "before-user",
            )],
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
    fn set_rolls_back_when_broadcast_fails() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "before")],
        );
        harness.state.lock().unwrap().fail_next_broadcast = true;

        let result = harness.service.set_variable(EnvironmentVariableInput {
            original_name: Some("JAVA_HOME".to_owned()),
            ..input(EnvironmentScope::User, "JAVA_HOME", "after")
        });

        assert!(matches!(result, Err(EnvironmentServiceError::Store(_))));
        let state = harness.state.lock().unwrap();
        assert_eq!(state.variables[&EnvironmentScope::User][0].value, "before");
        assert_eq!(state.broadcasts, 1);
    }

    #[test]
    fn set_rolls_back_when_snapshot_registry_read_fails() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "before")],
        );
        harness.state.lock().unwrap().fail_list_after_next_mutation = true;

        let result = harness.service.set_variable(EnvironmentVariableInput {
            original_name: Some("JAVA_HOME".to_owned()),
            ..input(EnvironmentScope::User, "JAVA_HOME", "after")
        });

        assert!(matches!(result, Err(EnvironmentServiceError::Store(_))));
        let state = harness.state.lock().unwrap();
        assert_eq!(state.variables[&EnvironmentScope::User][0].value, "before");
        assert_eq!(state.broadcasts, 2);
    }

    #[test]
    fn set_rolls_back_when_snapshot_backup_listing_fails() {
        let harness = service(false);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "before")],
        );
        std::fs::create_dir_all(&harness.directory).unwrap();
        std::fs::write(harness.directory.join("invalid.json"), b"not json").unwrap();

        let result = harness.service.set_variable(EnvironmentVariableInput {
            original_name: Some("JAVA_HOME".to_owned()),
            ..input(EnvironmentScope::User, "JAVA_HOME", "after")
        });

        assert!(matches!(result, Err(EnvironmentServiceError::Backup(_))));
        let state = harness.state.lock().unwrap();
        assert_eq!(state.variables[&EnvironmentScope::User][0].value, "before");
        assert_eq!(state.broadcasts, 2);
    }

    #[test]
    fn multi_scope_undo_rolls_back_both_scopes_when_finalization_fails() {
        let harness = service(true);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(
                EnvironmentScope::User,
                "USER_VALUE",
                "before-user",
            )],
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
        harness.state.lock().unwrap().fail_next_broadcast = true;

        let result = harness.service.undo_mutation(&[
            user_change.undo_backup_ids[0].clone(),
            system_change.undo_backup_ids[0].clone(),
        ]);

        assert!(matches!(result, Err(EnvironmentServiceError::Store(_))));
        let state = harness.state.lock().unwrap();
        assert_eq!(
            state.variables[&EnvironmentScope::User][0].value,
            "after-user"
        );
        assert_eq!(
            state.variables[&EnvironmentScope::System][0].value,
            "after-system"
        );
    }

    #[test]
    fn move_rolls_back_both_scopes_when_finalization_fails() {
        let harness = service(true);
        seed(
            &harness.state,
            EnvironmentScope::User,
            vec![variable(EnvironmentScope::User, "JAVA_HOME", "source")],
        );
        seed(
            &harness.state,
            EnvironmentScope::System,
            vec![variable(
                EnvironmentScope::System,
                "JAVA_HOME",
                "destination",
            )],
        );
        harness.state.lock().unwrap().fail_next_broadcast = true;

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
