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
