pub mod api;
#[cfg(desktop)]
pub mod desktop;
pub mod domain;
pub mod platform;
pub mod services;

use crate::services::backup::BackupStore;
use crate::services::command_shim::CommandShimStore;
use crate::services::environment::EnvironmentService;
use crate::services::settings::SettingsStore;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let backups = BackupStore::from_default_location()
        .expect("the environment manager backup directory must be available");
    let settings = SettingsStore::from_default_location()
        .expect("the environment manager settings path must be available");
    let command_shims = CommandShimStore::from_default_locations()
        .expect("the environment manager Command Shim paths must be available");
    let app_state = api::AppState::new(
        EnvironmentService::new(platform::system_store(), backups),
        settings,
        command_shims,
    );

    let builder = tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            api::get_environment_snapshot,
            api::save_environment_variable,
            api::delete_environment_variable,
            api::restore_environment_backup,
            api::undo_environment_mutation,
            api::transfer_environment_variable,
            api::get_environment_revision,
            api::launch_powershell,
            api::preview_environment_import,
            api::apply_environment_import,
            api::export_environment_file,
            api::get_favorites,
            api::toggle_favorite,
            api::analyze_path_entries,
            api::restart_elevated,
            api::get_command_shims,
            api::save_command_shim,
            api::repair_command_shims,
            api::delete_command_shim,
        ]);

    #[cfg(desktop)]
    let builder = builder
        .setup(desktop::configure)
        .on_window_event(desktop::handle_window_event);

    builder
        .run(tauri::generate_context!())
        .expect("error while running environment manager");
}
