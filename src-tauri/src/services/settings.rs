#[cfg(test)]
mod tests {
    use super::{FavoriteKey, SettingsStore};
    use crate::domain::environment::{
        EnvironmentScope, EnvironmentValueType, EnvironmentVariable, compare_variable_names,
    };
    use serde_json::{Value, json};
    use std::cmp::Ordering;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    const SECRET_VALUE: &str = "real-secret-must-never-be-persisted";

    #[test]
    fn missing_settings_file_lists_empty_without_creating_any_path() {
        let directory = TempDirectory::new("missing");
        let settings_directory = directory.path().join("nested");
        let store = SettingsStore::new(settings_directory.join("settings.json"));

        assert_eq!(store.list().unwrap(), Vec::<FavoriteKey>::new());
        assert!(!settings_directory.exists());
    }

    #[test]
    fn toggle_uses_registry_identity_and_keeps_scopes_independent() {
        let directory = TempDirectory::new("identity");
        let store = SettingsStore::new(directory.path().join("settings.json"));

        assert_eq!(
            store
                .toggle(favorite(EnvironmentScope::User, "ÄPFEL"))
                .unwrap(),
            vec![favorite(EnvironmentScope::User, "ÄPFEL")]
        );
        assert!(
            store
                .toggle(favorite(EnvironmentScope::User, "äpfel"))
                .unwrap()
                .is_empty()
        );

        store
            .toggle(favorite(EnvironmentScope::User, "Path"))
            .unwrap();
        assert_eq!(
            store
                .toggle(favorite(EnvironmentScope::System, "PATH"))
                .unwrap(),
            vec![
                favorite(EnvironmentScope::User, "Path"),
                favorite(EnvironmentScope::System, "PATH"),
            ]
        );

        store
            .toggle(favorite(EnvironmentScope::User, "Σ"))
            .unwrap();
        let favorites = store
            .toggle(favorite(EnvironmentScope::User, "ς"))
            .unwrap();
        if compare_variable_names("Σ", "ς") == Ordering::Equal {
            assert!(!favorites.contains(&favorite(EnvironmentScope::User, "Σ")));
            assert!(!favorites.contains(&favorite(EnvironmentScope::User, "ς")));
        } else {
            assert!(favorites.contains(&favorite(EnvironmentScope::User, "Σ")));
            assert!(favorites.contains(&favorite(EnvironmentScope::User, "ς")));
        }
    }

    #[test]
    fn persisted_document_contains_only_favorite_identity_and_round_trips() {
        let directory = TempDirectory::new("roundtrip");
        let path = directory.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        store
            .toggle(favorite(EnvironmentScope::User, "API_TOKEN"))
            .unwrap();

        store
            .reconcile(
                &[variable(
                    EnvironmentScope::User,
                    "API_TOKEN",
                    SECRET_VALUE,
                )],
                &[],
            )
            .unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(document["schemaVersion"], 1);
        assert_eq!(document["favorites"].as_array().unwrap().len(), 1);
        let persisted_favorite = document["favorites"][0].as_object().unwrap();
        assert_eq!(persisted_favorite.len(), 2);
        assert_eq!(persisted_favorite["scope"], "user");
        assert_eq!(persisted_favorite["name"], "API_TOKEN");
        assert!(!persisted_favorite.contains_key("value"));
        assert!(!String::from_utf8_lossy(&bytes).contains(SECRET_VALUE));

        assert_eq!(
            SettingsStore::new(path).list().unwrap(),
            vec![favorite(EnvironmentScope::User, "API_TOKEN")]
        );
    }

    #[test]
    fn list_has_deterministic_scope_then_registry_name_order() {
        let directory = TempDirectory::new("order");
        let path = directory.path().join("settings.json");
        write_document(
            &path,
            json!({
                "schemaVersion": 1,
                "favorites": [
                    { "scope": "system", "name": "A_SYSTEM" },
                    { "scope": "user", "name": "z_user" },
                    { "scope": "system", "name": "Z_SYSTEM" },
                    { "scope": "user", "name": "A_user" },
                    { "scope": "user", "name": "ς" },
                    { "scope": "user", "name": "Σ" }
                ]
            }),
        );
        let store = SettingsStore::new(path);

        let actual = store.list().unwrap();
        let mut expected = vec![
            favorite(EnvironmentScope::System, "A_SYSTEM"),
            favorite(EnvironmentScope::User, "z_user"),
            favorite(EnvironmentScope::System, "Z_SYSTEM"),
            favorite(EnvironmentScope::User, "A_user"),
            favorite(EnvironmentScope::User, "ς"),
            favorite(EnvironmentScope::User, "Σ"),
        ];
        expected.sort_by(compare_favorites);

        assert_eq!(actual, expected);
        assert!(actual[..4]
            .iter()
            .all(|favorite| favorite.scope == EnvironmentScope::User));
    }

    #[test]
    fn duplicate_registry_identity_in_one_scope_is_rejected() {
        let directory = TempDirectory::new("duplicate");
        let path = directory.path().join("settings.json");
        write_document(
            &path,
            json!({
                "schemaVersion": 1,
                "favorites": [
                    { "scope": "user", "name": "Path" },
                    { "scope": "user", "name": "PATH" }
                ]
            }),
        );

        assert_error_contains(
            SettingsStore::new(path).list(),
            &["duplicate", "favorite"],
        );
    }

    #[test]
    fn malformed_unsupported_and_oversized_files_are_rejected_without_overwrite() {
        for (label, original, expected) in [
            ("corrupt", b"{not-json".to_vec(), vec!["json"]),
            (
                "schema",
                br#"{"schemaVersion":2,"favorites":[]}"#.to_vec(),
                vec!["schema", "2"],
            ),
            ("oversized", vec![b' '; 1024 * 1024 + 1], vec!["large"]),
        ] {
            let directory = TempDirectory::new(label);
            let path = directory.path().join("settings.json");
            std::fs::write(&path, &original).unwrap();
            let store = SettingsStore::new(path.clone());

            assert_error_contains(store.list(), &expected);
            assert_error_contains(
                store.toggle(favorite(EnvironmentScope::User, "NEW_VALUE")),
                &expected,
            );
            assert_error_contains(store.reconcile(&[], &[]), &expected);
            assert_eq!(std::fs::read(path).unwrap(), original);
        }
    }

    #[test]
    fn invalid_favorite_names_are_rejected_without_changing_existing_file() {
        let directory = TempDirectory::new("invalid-name");
        let path = directory.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        store
            .toggle(favorite(EnvironmentScope::User, "KEEP_ME"))
            .unwrap();
        let original = std::fs::read(&path).unwrap();

        for invalid in ["", "HAS=EQUALS", "HAS\0NUL"] {
            assert_error_contains(
                store.toggle(favorite(EnvironmentScope::User, invalid)),
                &["name"],
            );
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn invalid_favorites_loaded_from_disk_are_rejected_without_overwrite() {
        let directory = TempDirectory::new("invalid-on-disk");
        let path = directory.path().join("settings.json");
        let original = br#"{"schemaVersion":1,"favorites":[{"scope":"user","name":"BAD\u0000NAME"}]}"#;
        std::fs::write(&path, original).unwrap();
        let store = SettingsStore::new(path.clone());

        assert_error_contains(store.list(), &["name"]);
        assert_error_contains(store.reconcile(&[], &[]), &["name"]);
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn reconcile_removes_missing_entries_and_adopts_registry_display_names() {
        let directory = TempDirectory::new("reconcile");
        let path = directory.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        store
            .toggle(favorite(EnvironmentScope::User, "path"))
            .unwrap();
        store
            .toggle(favorite(EnvironmentScope::System, "PATH"))
            .unwrap();
        store
            .toggle(favorite(EnvironmentScope::System, "MISSING"))
            .unwrap();

        let reconciled = store
            .reconcile(
                &[variable(EnvironmentScope::User, "Path", SECRET_VALUE)],
                &[variable(EnvironmentScope::System, "Path", "system value")],
            )
            .unwrap();

        assert_eq!(
            reconciled,
            vec![
                favorite(EnvironmentScope::User, "Path"),
                favorite(EnvironmentScope::System, "Path"),
            ]
        );
        assert_eq!(store.list().unwrap(), reconciled);
        let serialized = std::fs::read_to_string(path).unwrap();
        assert!(!serialized.contains("MISSING"));
        assert!(!serialized.contains(SECRET_VALUE));
    }

    #[test]
    fn reconcile_without_changes_preserves_document_bytes() {
        let directory = TempDirectory::new("reconcile-no-change");
        let path = directory.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        store
            .toggle(favorite(EnvironmentScope::User, "JAVA_HOME"))
            .unwrap();
        let before = std::fs::read(&path).unwrap();

        let favorites = store
            .reconcile(
                &[variable(
                    EnvironmentScope::User,
                    "JAVA_HOME",
                    r"C:\Java",
                )],
                &[],
            )
            .unwrap();

        assert_eq!(
            favorites,
            vec![favorite(EnvironmentScope::User, "JAVA_HOME")]
        );
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn successful_toggle_atomically_replaces_target_without_temp_files() {
        let directory = TempDirectory::new("atomic");
        let path = directory.path().join("settings.json");
        write_document(
            &path,
            json!({
                "schemaVersion": 1,
                "favorites": [{ "scope": "user", "name": "OLD" }]
            }),
        );
        let old_bytes = std::fs::read(&path).unwrap();
        let store = SettingsStore::new(path.clone());

        store
            .toggle(favorite(EnvironmentScope::User, "NEW"))
            .unwrap();

        assert_ne!(std::fs::read(&path).unwrap(), old_bytes);
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")
        }));
    }

    #[test]
    #[cfg(windows)]
    fn default_location_is_appdata_env_manager_settings_without_writing() {
        let app_data = std::env::var_os("APPDATA").expect("APPDATA must be available on Windows");
        let expected = PathBuf::from(app_data)
            .join("EnvManager")
            .join("settings.json");

        let store = SettingsStore::from_default_location().unwrap();

        assert_eq!(store.path(), expected.as_path());
    }

    fn favorite(scope: EnvironmentScope, name: &str) -> FavoriteKey {
        FavoriteKey {
            scope,
            name: name.to_owned(),
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

    fn compare_favorites(left: &FavoriteKey, right: &FavoriteKey) -> Ordering {
        scope_order(left.scope)
            .cmp(&scope_order(right.scope))
            .then_with(|| compare_variable_names(&left.name, &right.name))
            .then_with(|| left.name.cmp(&right.name))
    }

    fn scope_order(scope: EnvironmentScope) -> u8 {
        match scope {
            EnvironmentScope::User => 0,
            EnvironmentScope::System => 1,
        }
    }

    fn write_document(path: &Path, document: Value) {
        std::fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    }

    fn assert_error_contains<T, E: std::fmt::Display>(result: Result<T, E>, expected: &[&str]) {
        let message = match result {
            Ok(_) => panic!("expected settings operation to fail"),
            Err(error) => error.to_string().to_lowercase(),
        };
        for fragment in expected {
            assert!(
                message.contains(&fragment.to_lowercase()),
                "expected error '{message}' to contain '{fragment}'"
            );
        }
    }

    struct TempDirectory {
        path: PathBuf,
    }

    impl TempDirectory {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "env-manager-settings-{label}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
