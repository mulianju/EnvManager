#![cfg(windows)]

use env_manager_lib::domain::environment::{
    EnvironmentScope, EnvironmentValueType, EnvironmentVariableInput,
};
use env_manager_lib::platform::system_store;
use env_manager_lib::services::backup::BackupStore;
use env_manager_lib::services::environment::EnvironmentService;
use std::time::{SystemTime, UNIX_EPOCH};

struct RegistryCleanup {
    name: String,
}

impl Drop for RegistryCleanup {
    fn drop(&mut self) {
        let store = system_store();
        let _ = store.delete(EnvironmentScope::User, &self.name);
        let _ = store.broadcast_change();
    }
}

#[test]
#[ignore = "writes a unique temporary HKCU environment value"]
fn writes_reads_and_deletes_a_temporary_user_variable() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("CODEX_ENVMANAGER_TEST_{nonce}");
    let cleanup = RegistryCleanup { name: name.clone() };
    let backup_directory = std::env::temp_dir().join(format!("env-manager-live-test-{nonce}"));
    let service =
        EnvironmentService::new(system_store(), BackupStore::new(backup_directory.clone()));

    let snapshot = service
        .set_variable(EnvironmentVariableInput {
            original_name: None,
            name: name.clone(),
            value: "temporary-value".to_owned(),
            value_type: EnvironmentValueType::String,
            scope: EnvironmentScope::User,
        })
        .unwrap();
    assert!(
        snapshot
            .user_variables
            .iter()
            .any(|variable| { variable.name == name && variable.value == "temporary-value" })
    );

    let snapshot = service
        .delete_variable(EnvironmentScope::User, &name)
        .unwrap();
    assert!(
        !snapshot
            .user_variables
            .iter()
            .any(|variable| variable.name == name)
    );

    drop(cleanup);
    std::fs::remove_dir_all(backup_directory).unwrap();
}
