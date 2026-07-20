use crate::domain::environment::{EnvironmentScope, EnvironmentVariableInput};
use crate::platform::{EnvironmentStoreError, restart_as_administrator};
use crate::services::environment::{
    EnvironmentService, EnvironmentServiceError, EnvironmentSnapshot, MutationResult,
    PathEntryStatus,
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
            EnvironmentServiceError::UndoInvalid(_) => "invalidUndo",
            EnvironmentServiceError::TransactionRollbackFailed(_) => "transactionRollbackFailed",
            EnvironmentServiceError::Store(EnvironmentStoreError::AccessDenied) => {
                "elevationRequired"
            }
            EnvironmentServiceError::Store(_) => "registryOperationFailed",
            EnvironmentServiceError::Backup(_) => "backupOperationFailed",
            EnvironmentServiceError::TransferFile(_) => "transferFileOperationFailed",
        };
        Self::new(code, error)
    }
}

pub struct AppState {
    service: Mutex<EnvironmentService>,
}

impl AppState {
    pub fn new(service: EnvironmentService) -> Self {
        Self {
            service: Mutex::new(service),
        }
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
    state: State<'_, AppState>,
) -> Result<MutationResult, ApiError> {
    lock_service(&state)?
        .set_variable(input)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn delete_environment_variable(
    scope: EnvironmentScope,
    name: String,
    state: State<'_, AppState>,
) -> Result<MutationResult, ApiError> {
    lock_service(&state)?
        .delete_variable(scope, &name)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn restore_environment_backup(
    backup_id: String,
    state: State<'_, AppState>,
) -> Result<MutationResult, ApiError> {
    lock_service(&state)?
        .restore_backup(&backup_id)
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

fn lock_service<'a>(
    state: &'a State<'_, AppState>,
) -> Result<MutexGuard<'a, EnvironmentService>, ApiError> {
    state
        .service
        .lock()
        .map_err(|_| ApiError::new("serviceLockFailed", "Environment service access failed."))
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
            json!({ "token": "preview-token", "items": [] })
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
