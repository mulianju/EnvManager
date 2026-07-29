use tauri_build::{AppManifest, Attributes};

const COMMANDS: &[&str] = &[
    "get_environment_snapshot",
    "save_environment_variable",
    "delete_environment_variable",
    "restore_environment_backup",
    "undo_environment_mutation",
    "transfer_environment_variable",
    "get_environment_revision",
    "launch_powershell",
    "preview_environment_import",
    "apply_environment_import",
    "export_environment_file",
    "get_favorites",
    "toggle_favorite",
    "analyze_path_entries",
    "restart_elevated",
    "get_command_shims",
    "save_command_shim",
    "delete_command_shim",
];

fn main() {
    tauri_build::try_build(Attributes::new().app_manifest(AppManifest::new().commands(COMMANDS)))
        .expect("failed to run the Tauri build script");
}
