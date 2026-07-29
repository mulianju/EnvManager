use crate::domain::environment::{
    EnvironmentScope, EnvironmentVariableInput, TransferVariableInput, validate_variable_name,
    variable_names_equal,
};
use crate::platform::{EnvironmentStoreError, restart_as_administrator};
use crate::services::command_shim::{
    CommandShimError, CommandShimInput, CommandShimSnapshot, CommandShimStore,
};
use crate::services::environment::{
    EnvironmentService, EnvironmentServiceError, EnvironmentSnapshot, MutationResult,
    PathEntryStatus,
};
use crate::services::settings::{FavoriteKey, SettingsError, SettingsStore};
use crate::services::transfer_file::{
    ExportFileRequest, ExportSummary, ImportConflictStrategy, ImportFileRequest, ImportPreview,
};
use serde::Serialize;
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl ApiError {
    fn new(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_string(),
        }
    }
}

impl From<EnvironmentServiceError> for ApiError {
    fn from(error: EnvironmentServiceError) -> Self {
        let code = match &error {
            EnvironmentServiceError::Validation(_) => "invalidVariable",
            EnvironmentServiceError::ElevationRequired => "elevationRequired",
            EnvironmentServiceError::VariableAlreadyExists(_) => "variableAlreadyExists",
            EnvironmentServiceError::VariableNotFound(_) => "variableNotFound",
            EnvironmentServiceError::InvalidTransfer(_) => "invalidTransfer",
            EnvironmentServiceError::ImportPreviewChanged => "importPreviewChanged",
            EnvironmentServiceError::EnvironmentChanged => "environmentChanged",
            EnvironmentServiceError::UndoInvalid(_) => "invalidUndo",
            EnvironmentServiceError::TransactionRollbackFailed(_) => "transactionRollbackFailed",
            EnvironmentServiceError::Store(EnvironmentStoreError::AccessDenied) => {
                "elevationRequired"
            }
            EnvironmentServiceError::Store(_) => "registryOperationFailed",
            EnvironmentServiceError::Backup(_) => "backupOperationFailed",
            EnvironmentServiceError::TransferFile(_) => "importExportFailed",
        };
        Self::new(code, error)
    }
}

impl From<SettingsError> for ApiError {
    fn from(error: SettingsError) -> Self {
        Self::new("settingsOperationFailed", error)
    }
}

impl From<CommandShimError> for ApiError {
    fn from(error: CommandShimError) -> Self {
        let code = match &error {
            CommandShimError::InvalidCommandName(_) => "invalidCommandName",
            CommandShimError::InvalidExecutable(_)
            | CommandShimError::MissingExecutable(_)
            | CommandShimError::InvalidFixedArgument(_)
            | CommandShimError::MissingTarget(_) => "shimTargetMissing",
            CommandShimError::DuplicateCommandName(_) | CommandShimError::NameConflict(_) => {
                "shimConflict"
            }
            CommandShimError::ExternallyModified(_) => "shimExternallyModified",
            CommandShimError::UnsupportedPlatform => "unsupportedPlatform",
            _ => "shimOperationFailed",
        };
        Self::new(code, error)
    }
}

pub struct AppState {
    service: Mutex<EnvironmentService>,
    settings: SettingsStore,
    command_shims: CommandShimStore,
}

impl AppState {
    pub fn new(
        service: EnvironmentService,
        settings: SettingsStore,
        command_shims: CommandShimStore,
    ) -> Self {
        Self {
            service: Mutex::new(service),
            settings,
            command_shims,
        }
    }

    pub fn launch_powershell(&self) -> Result<(), ApiError> {
        self.lock_service()?
            .launch_powershell()
            .map_err(ApiError::from)
    }

    fn lock_service(&self) -> Result<MutexGuard<'_, EnvironmentService>, ApiError> {
        self.service
            .lock()
            .map_err(|_| ApiError::new("serviceLockFailed", "Environment service access failed."))
    }
}

#[tauri::command]
pub fn get_environment_snapshot(
    state: State<'_, AppState>,
) -> Result<EnvironmentSnapshot, ApiError> {
    lock_service(&state)?.snapshot().map_err(ApiError::from)
}

#[tauri::command]
pub fn save_environment_variable(
    input: EnvironmentVariableInput,
    expected_revision: String,
    state: State<'_, AppState>,
) -> Result<MutationResult, ApiError> {
    lock_service(&state)?
        .set_variable_checked(input, &expected_revision)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn delete_environment_variable(
    scope: EnvironmentScope,
    name: String,
    expected_revision: String,
    state: State<'_, AppState>,
) -> Result<MutationResult, ApiError> {
    lock_service(&state)?
        .delete_variable_checked(scope, &name, &expected_revision)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn restore_environment_backup(
    backup_id: String,
    expected_revision: String,
    state: State<'_, AppState>,
) -> Result<MutationResult, ApiError> {
    lock_service(&state)?
        .restore_backup_checked(&backup_id, &expected_revision)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn undo_environment_mutation(
    backup_ids: Vec<String>,
    expected_revision: String,
    state: State<'_, AppState>,
) -> Result<MutationResult, ApiError> {
    lock_service(&state)?
        .undo_mutation_checked(&backup_ids, &expected_revision)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn transfer_environment_variable(
    input: TransferVariableInput,
    expected_revision: String,
    state: State<'_, AppState>,
) -> Result<MutationResult, ApiError> {
    lock_service(&state)?
        .transfer_variable_checked(input, &expected_revision)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn get_environment_revision(state: State<'_, AppState>) -> Result<String, ApiError> {
    lock_service(&state)?.revision().map_err(ApiError::from)
}

#[tauri::command]
pub fn launch_powershell(state: State<'_, AppState>) -> Result<(), ApiError> {
    state.launch_powershell()
}

#[tauri::command]
pub fn preview_environment_import(
    request: ImportFileRequest,
    state: State<'_, AppState>,
) -> Result<ImportPreview, ApiError> {
    lock_service(&state)?
        .preview_import(&request)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn apply_environment_import(
    request: ImportFileRequest,
    strategy: ImportConflictStrategy,
    expected_token: String,
    expected_revision: String,
    state: State<'_, AppState>,
) -> Result<MutationResult, ApiError> {
    lock_service(&state)?
        .apply_import(&request, strategy, &expected_token, &expected_revision)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn export_environment_file(
    request: ExportFileRequest,
    state: State<'_, AppState>,
) -> Result<ExportSummary, ApiError> {
    lock_service(&state)?
        .export_file(&request)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn get_favorites(state: State<'_, AppState>) -> Result<Vec<FavoriteKey>, ApiError> {
    let snapshot = lock_service(&state)?.snapshot().map_err(ApiError::from)?;
    state
        .settings
        .reconcile(&snapshot.user_variables, &snapshot.system_variables)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn toggle_favorite(
    favorite: FavoriteKey,
    state: State<'_, AppState>,
) -> Result<Vec<FavoriteKey>, ApiError> {
    validate_variable_name(&favorite.name)
        .map_err(|error| ApiError::from(SettingsError::Validation(error)))?;
    let service = lock_service(&state)?;
    let snapshot = service.snapshot().map_err(ApiError::from)?;
    let variables = match favorite.scope {
        EnvironmentScope::User => &snapshot.user_variables,
        EnvironmentScope::System => &snapshot.system_variables,
    };
    let variable = variables
        .iter()
        .find(|variable| variable_names_equal(&variable.name, &favorite.name))
        .ok_or_else(|| {
            ApiError::from(EnvironmentServiceError::VariableNotFound(
                favorite.name.clone(),
            ))
        })?;
    state
        .settings
        .toggle(FavoriteKey {
            scope: favorite.scope,
            name: variable.name.clone(),
        })
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn analyze_path_entries(
    entries: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<PathEntryStatus>, ApiError> {
    lock_service(&state)?
        .analyze_path_entries(&entries)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn restart_elevated(app: AppHandle) -> Result<(), ApiError> {
    restart_as_administrator().map_err(|error| ApiError::new("elevationFailed", error))?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn get_command_shims(state: State<'_, AppState>) -> Result<CommandShimSnapshot, ApiError> {
    #[cfg(not(windows))]
    {
        let _ = state;
        return Err(ApiError::from(CommandShimError::UnsupportedPlatform));
    }
    #[cfg(windows)]
    {
        state.command_shims.transaction(|| {
            let service = lock_service(&state)?;
            let path_ready = service
                .user_path_contains(state.command_shims.managed_directory())
                .map_err(ApiError::from)?;
            let search_path = service.effective_path_entries().map_err(ApiError::from)?;
            state
                .command_shims
                .snapshot_with_path(path_ready, &search_path)
                .map_err(ApiError::from)
        })
    }
}

#[tauri::command]
pub fn save_command_shim(
    input: CommandShimInput,
    state: State<'_, AppState>,
) -> Result<CommandShimSnapshot, ApiError> {
    #[cfg(not(windows))]
    {
        let _ = (input, state);
        return Err(ApiError::from(CommandShimError::UnsupportedPlatform));
    }
    #[cfg(windows)]
    {
        state.command_shims.transaction(|| {
            let service = lock_service(&state)?;
            let search_path = service.effective_path_entries().map_err(ApiError::from)?;
            state
                .command_shims
                .preflight_save(&input, &search_path)
                .map_err(ApiError::from)?;
            let path_mutation = service
                .ensure_user_path_entry_transactional(state.command_shims.managed_directory())
                .map_err(ApiError::from)?;
            match state
                .command_shims
                .save_with_path(input, &search_path, true)
            {
                Ok(snapshot) => Ok(snapshot),
                Err(error) => {
                    if let Some(mutation) = path_mutation
                        && let Err(rollback_error) = service.rollback_user_path_entry(mutation)
                    {
                        return Err(ApiError::new(
                            "shimOperationFailed",
                            format!("{error} User Path rollback also failed: {rollback_error}"),
                        ));
                    }
                    Err(ApiError::from(error))
                }
            }
        })
    }
}

#[tauri::command]
pub fn delete_command_shim(
    id: String,
    state: State<'_, AppState>,
) -> Result<CommandShimSnapshot, ApiError> {
    #[cfg(not(windows))]
    {
        let _ = (id, state);
        return Err(ApiError::from(CommandShimError::UnsupportedPlatform));
    }
    #[cfg(windows)]
    {
        state.command_shims.transaction(|| {
            let service = lock_service(&state)?;
            let path_ready = service
                .user_path_contains(state.command_shims.managed_directory())
                .map_err(ApiError::from)?;
            let search_path = service.effective_path_entries().map_err(ApiError::from)?;
            state
                .command_shims
                .delete_with_path(&id, path_ready, &search_path)
                .map_err(ApiError::from)
        })
    }
}

fn lock_service<'a>(
    state: &'a State<'_, AppState>,
) -> Result<MutexGuard<'a, EnvironmentService>, ApiError> {
    state.lock_service()
}

#[cfg(test)]
mod tests {
    use super::ApiError;
    use crate::domain::environment::{
        EnvironmentScope, EnvironmentValidationError, TransferMode, TransferVariableInput,
    };
    use crate::platform::EnvironmentStoreError;
    use crate::services::environment::{
        EnvironmentServiceError, EnvironmentSnapshot, MutationResult,
    };
    use crate::services::settings::{FavoriteKey, SettingsError};
    use crate::services::transfer_file::{
        ExportFileRequest, ImportFileRequest, ImportPreview, TransferFileError, TransferFileFormat,
    };
    use serde_json::json;
    use std::io;
    use std::path::PathBuf;

    #[test]
    fn maps_service_errors_to_stable_api_codes() {
        let cases = [
            (
                EnvironmentServiceError::ImportPreviewChanged,
                "importPreviewChanged",
            ),
            (
                EnvironmentServiceError::EnvironmentChanged,
                "environmentChanged",
            ),
            (
                EnvironmentServiceError::InvalidTransfer("same scope".to_owned()),
                "invalidTransfer",
            ),
            (
                EnvironmentServiceError::UndoInvalid("missing backup".to_owned()),
                "invalidUndo",
            ),
            (
                EnvironmentServiceError::TransactionRollbackFailed("write failed".to_owned()),
                "transactionRollbackFailed",
            ),
            (
                EnvironmentServiceError::Store(EnvironmentStoreError::AccessDenied),
                "elevationRequired",
            ),
        ];

        for (error, expected_code) in cases {
            assert_eq!(ApiError::from(error).code, expected_code);
        }
    }

    #[test]
    fn maps_transfer_file_parse_and_file_errors_to_one_api_code() {
        let parse_error = EnvironmentServiceError::TransferFile(TransferFileError::InvalidFormat {
            format: TransferFileFormat::Json,
            line: Some(2),
            message: "Invalid JSON".to_owned(),
        });
        let file_error = EnvironmentServiceError::TransferFile(TransferFileError::Io(
            io::Error::new(io::ErrorKind::NotFound, "File not found"),
        ));

        assert_eq!(ApiError::from(parse_error).code, "importExportFailed");
        assert_eq!(ApiError::from(file_error).code, "importExportFailed");
    }

    #[test]
    fn maps_settings_errors_to_a_stable_api_code() {
        assert_eq!(
            ApiError::from(SettingsError::PathUnavailable).code,
            "settingsOperationFailed"
        );
    }

    #[test]
    fn api_error_messages_do_not_expose_variable_values() {
        let secret = "super-secret-value";
        let error = ApiError::from(EnvironmentServiceError::Validation(
            EnvironmentValidationError::InvalidValue,
        ));

        assert!(!error.message.contains(secret));
    }

    #[test]
    fn serializes_mutation_receipts_and_import_previews_in_camel_case() {
        let mutation = MutationResult {
            snapshot: empty_snapshot(),
            undo_backup_ids: vec!["user-backup.json".to_owned()],
        };
        let preview = ImportPreview {
            token: "preview-token".to_owned(),
            environment_revision: "preview-revision".to_owned(),
            items: Vec::new(),
        };

        let mutation_json = serde_json::to_value(mutation).unwrap();
        assert_eq!(
            mutation_json.get("undoBackupIds"),
            Some(&json!(["user-backup.json"]))
        );
        assert!(mutation_json.get("undo_backup_ids").is_none());
        assert_eq!(
            serde_json::to_value(preview).unwrap(),
            json!({
                "token": "preview-token",
                "environmentRevision": "preview-revision",
                "items": []
            })
        );
    }

    #[test]
    fn serializes_favorites_and_transfer_inputs_in_camel_case() {
        let favorite = FavoriteKey {
            scope: EnvironmentScope::User,
            name: "JAVA_HOME".to_owned(),
        };
        let transfer = TransferVariableInput {
            source_scope: EnvironmentScope::User,
            target_scope: EnvironmentScope::System,
            name: "JAVA_HOME".to_owned(),
            mode: TransferMode::Move,
            overwrite: true,
        };

        assert_eq!(
            serde_json::to_value(favorite).unwrap(),
            json!({ "scope": "user", "name": "JAVA_HOME" })
        );
        assert_eq!(
            serde_json::to_value(transfer).unwrap(),
            json!({
                "sourceScope": "user",
                "targetScope": "system",
                "name": "JAVA_HOME",
                "mode": "move",
                "overwrite": true
            })
        );
    }

    #[test]
    fn serializes_import_and_export_requests_in_camel_case() {
        let import = ImportFileRequest {
            path: PathBuf::from(r"C:\temp\variables.json"),
            format: TransferFileFormat::Json,
            default_scope: Some(EnvironmentScope::User),
        };
        let export = ExportFileRequest {
            path: PathBuf::from(r"C:\temp\variables.reg"),
            format: TransferFileFormat::Registry,
            scope: Some(EnvironmentScope::System),
        };

        assert_eq!(
            serde_json::to_value(import).unwrap(),
            json!({
                "path": r"C:\temp\variables.json",
                "format": "json",
                "defaultScope": "user"
            })
        );
        assert_eq!(
            serde_json::to_value(export).unwrap(),
            json!({
                "path": r"C:\temp\variables.reg",
                "format": "registry",
                "scope": "system"
            })
        );
    }

    fn empty_snapshot() -> EnvironmentSnapshot {
        EnvironmentSnapshot {
            user_variables: Vec::new(),
            system_variables: Vec::new(),
            effective_variables: Vec::new(),
            revision: "revision".to_owned(),
            is_elevated: false,
            backups: Vec::new(),
            backup_directory: PathBuf::from("backups"),
        }
    }
}
