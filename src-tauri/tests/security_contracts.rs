use serde_json::Value;
use std::collections::BTreeSet;

const ALL_APP_COMMANDS: &[&str] = &[
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
];

const QUICK_READ_COMMANDS: &[&str] = &[
    "get_environment_snapshot",
    "get_environment_revision",
    "get_favorites",
];

#[test]
fn build_manifest_registers_every_invokable_application_command() {
    let build_script = include_str!("../build.rs");

    assert!(
        build_script.contains("AppManifest"),
        "build.rs must register a Tauri AppManifest so custom commands participate in ACL checks"
    );
    assert!(
        build_script.contains(".app_manifest("),
        "the AppManifest must be attached to tauri_build::Attributes"
    );
    assert!(
        build_script.contains(".commands("),
        "the AppManifest must generate permissions for application commands"
    );
    for command in ALL_APP_COMMANDS {
        assert!(
            build_script.contains(&format!("\"{command}\"")),
            "the AppManifest is missing command {command}"
        );
    }
}

#[test]
fn main_window_capability_allows_every_application_command() {
    let permissions = capability_permissions(include_str!("../capabilities/default.json"));

    for command in ALL_APP_COMMANDS {
        let permission = command_permission(command);
        assert!(
            permissions.contains(&permission),
            "main capability is missing {permission} for command {command}"
        );
    }
}

#[test]
fn quick_window_capability_exposes_only_read_only_application_commands() {
    let permissions = capability_permissions(include_str!("../capabilities/quick.json"));
    let app_permissions = permissions
        .iter()
        .filter(|permission| permission.starts_with("allow-"))
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected = QUICK_READ_COMMANDS
        .iter()
        .map(|command| command_permission(command))
        .collect::<BTreeSet<_>>();

    assert_eq!(
        app_permissions, expected,
        "quick must not receive any mutating application command permission"
    );
    assert!(permissions.contains("clipboard-manager:allow-write-text"));
}

#[test]
fn toggle_favorite_holds_the_environment_service_guard_through_settings_update() {
    let api_source = include_str!("../src/api.rs");
    let function = source_between(
        api_source,
        "pub fn toggle_favorite(",
        "pub fn analyze_path_entries(",
    );
    let guard_line = function
        .lines()
        .find(|line| line.contains("= lock_service(&state)?;"))
        .expect(
            "toggle_favorite must bind the EnvironmentService MutexGuard before checking existence",
        );
    let guard_name = guard_line
        .trim()
        .strip_prefix("let ")
        .and_then(|line| line.split_once('='))
        .map(|(name, _)| name.trim())
        .expect("the service guard must be a named local binding");
    let snapshot_use = format!("{guard_name}.snapshot()");

    assert!(
        function.contains(&snapshot_use),
        "the existence snapshot must be read through the named service guard"
    );
    assert!(
        function.find(guard_line.trim()).unwrap() < function.find(".settings").unwrap(),
        "the service guard must be acquired before the settings write"
    );
    assert!(
        !function.contains(&format!("drop({guard_name})")),
        "the service guard must remain in scope until settings.toggle completes"
    );
}

fn capability_permissions(source: &str) -> BTreeSet<String> {
    serde_json::from_str::<Value>(source)
        .expect("capability must be valid JSON")
        .get("permissions")
        .and_then(Value::as_array)
        .expect("capability permissions must be an array")
        .iter()
        .map(|permission| {
            permission
                .as_str()
                .expect("capability permission must be a string")
                .to_owned()
        })
        .collect()
}

fn command_permission(command: &str) -> String {
    format!("allow-{}", command.replace('_', "-"))
}

fn source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    source
        .split_once(start)
        .map(|(_, remainder)| remainder)
        .and_then(|remainder| remainder.split_once(end).map(|(body, _)| body))
        .expect("expected function markers in source")
}
