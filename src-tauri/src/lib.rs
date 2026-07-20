pub mod api;
pub mod domain;
pub mod platform;
pub mod services;

use crate::services::backup::BackupStore;
use crate::services::environment::EnvironmentService;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let backups = BackupStore::from_default_location()
        .expect("the environment manager backup directory must be available");
    let app_state = api::AppState::new(EnvironmentService::new(platform::system_store(), backups));

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            api::get_environment_snapshot,
            api::save_environment_variable,
            api::delete_environment_variable,
            api::restore_environment_backup,
            api::analyze_path_entries,
            api::restart_elevated,
        ])
        .run(tauri::generate_context!())
        .expect("error while running environment manager");
}
