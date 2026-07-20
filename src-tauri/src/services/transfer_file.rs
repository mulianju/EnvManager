#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::environment::{
        EnvironmentScope, EnvironmentValueType, EnvironmentVariable,
    };
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "env-manager-transfer-{label}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            if self.0.exists() {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }

    fn variable(
        scope: EnvironmentScope,
        name: &str,
        value: &str,
        value_type: EnvironmentValueType,
    ) -> EnvironmentVariable {
        EnvironmentVariable {
            name: name.to_owned(),
            value: value.to_owned(),
            value_type,
            scope,
        }
    }

    fn assert_error_contains<T, E: std::fmt::Display>(result: Result<T, E>, expected: &str) {
        let error = result.err().expect("expected an error").to_string();
        assert!(
            error.to_ascii_lowercase().contains(&expected.to_ascii_lowercase()),
            "expected error containing {expected:?}, got {error:?}"
        );
    }

    fn decode_registry(bytes: &[u8]) -> String {
        assert_eq!(&bytes[..2], &[0xff, 0xfe]);
        let words = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&words).unwrap()
    }

    fn encode_utf16le_bom(value: &str) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xfe];
        for word in value.encode_utf16() {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn json_schema_v1_round_trips_scope_type_and_value() {
        let expected = vec![
            variable(
                EnvironmentScope::User,
                "JAVA_HOME",
                r"C:\Java\jdk",
                EnvironmentValueType::String,
            ),
            variable(
                EnvironmentScope::System,
                "Path",
                r"%SystemRoot%\System32",
                EnvironmentValueType::ExpandableString,
            ),
        ];

        let bytes = serialize_export(TransferFileFormat::Json, &expected).unwrap();
        let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(document["schemaVersion"], 1);
        assert_eq!(
            parse_import_bytes(TransferFileFormat::Json, &bytes, None).unwrap(),
            expected
        );
    }

    #[test]
    fn json_rejects_unsupported_schema_duplicate_names_and_invalid_values() {
        assert_error_contains(
            parse_import_bytes(
                TransferFileFormat::Json,
                br#"{"schemaVersion":2,"variables":[]}"#,
                None,
            ),
            "schema",
        );
        assert_error_contains(
            parse_import_bytes(
                TransferFileFormat::Json,
                br#"{"schemaVersion":1,"variables":[{"name":"Path","value":"one","valueType":"string","scope":"user"},{"name":"PATH","value":"two","valueType":"string","scope":"user"}]}"#,
                None,
            ),
            "duplicate",
        );
        assert_error_contains(
            parse_import_bytes(
                TransferFileFormat::Json,
                b"{\"schemaVersion\":1,\"variables\":[{\"name\":\"A=B\",\"value\":\"ok\",\"valueType\":\"string\",\"scope\":\"user\"}]}",
                None,
            ),
            "name",
        );
        assert_error_contains(
            parse_import_bytes(
                TransferFileFormat::Json,
                b"{\"schemaVersion\":1,\"variables\":[{\"name\":\"VALID\",\"value\":\"bad\\u0000value\",\"valueType\":\"string\",\"scope\":\"user\"}]}",
                None,
            ),
            "value",
        );
    }

    #[test]
    fn json_allows_the_same_name_in_different_scopes() {
        let bytes = br#"{"schemaVersion":1,"variables":[{"name":"Path","value":"user","valueType":"string","scope":"user"},{"name":"PATH","value":"system","valueType":"expandableString","scope":"system"}]}"#;

        let variables = parse_import_bytes(TransferFileFormat::Json, bytes, None).unwrap();

        assert_eq!(variables.len(), 2);
    }

    #[test]
    fn dotenv_requires_a_default_scope_and_supports_common_syntax() {
        let bytes = br#"
# comment
export PLAIN=alpha
WITH_EQUALS=left=right
SINGLE='literal $PLAIN # value'
DOUBLE="line\nnext\rreturn\ttab\\slash\"quote"
EMPTY=
INLINE=value # trailing comment
NO_INTERPOLATION=$PLAIN/bin
"#;
        assert_error_contains(
            parse_import_bytes(TransferFileFormat::DotEnv, bytes, None),
            "scope",
        );

        let variables = parse_import_bytes(
            TransferFileFormat::DotEnv,
            bytes,
            Some(EnvironmentScope::User),
        )
        .unwrap();

        assert_eq!(variables.len(), 7);
        assert!(variables.iter().all(|variable| {
            variable.scope == EnvironmentScope::User
                && variable.value_type == EnvironmentValueType::String
        }));
        assert_eq!(variables[0].value, "alpha");
        assert_eq!(variables[1].value, "left=right");
        assert_eq!(variables[2].value, "literal $PLAIN # value");
        assert_eq!(
            variables[3].value,
            "line\nnext\rreturn\ttab\\slash\"quote"
        );
        assert_eq!(variables[4].value, "");
        assert_eq!(variables[5].value, "value");
        assert_eq!(variables[6].value, "$PLAIN/bin");
    }

    #[test]
    fn dotenv_reports_line_numbers_for_invalid_input() {
        for (bytes, message) in [
            (&b"GOOD=1\nBAD-KEY=2"[..], "line 2"),
            (&b"GOOD=1\nBROKEN='value"[..], "line 2"),
            (&b"Path=one\nPATH=two"[..], "line 2"),
            (&b"GOOD=1\nBAD=\xff"[..], "line 2"),
        ] {
            assert_error_contains(
                parse_import_bytes(
                    TransferFileFormat::DotEnv,
                    bytes,
                    Some(EnvironmentScope::User),
                ),
                message,
            );
        }
    }

    #[test]
    fn dotenv_export_is_deterministic_and_round_trips_special_characters() {
        let variables = vec![
            variable(
                EnvironmentScope::User,
                "Z_EMPTY",
                "",
                EnvironmentValueType::String,
            ),
            variable(
                EnvironmentScope::User,
                "A_SPECIAL",
                " spaces # equals= newline\nquote\" slash\\ dollar$ ",
                EnvironmentValueType::String,
            ),
        ];
        let reversed = variables.iter().cloned().rev().collect::<Vec<_>>();

        let bytes = serialize_export(TransferFileFormat::DotEnv, &variables).unwrap();

        assert_eq!(
            bytes,
            serialize_export(TransferFileFormat::DotEnv, &reversed).unwrap()
        );
        assert_eq!(
            parse_import_bytes(
                TransferFileFormat::DotEnv,
                &bytes,
                Some(EnvironmentScope::User),
            )
            .unwrap(),
            reversed
        );
    }

    #[test]
    fn dotenv_export_rejects_mixed_scopes_and_missing_explicit_scope() {
        let mixed = vec![
            variable(
                EnvironmentScope::User,
                "USER_VALUE",
                "one",
                EnvironmentValueType::String,
            ),
            variable(
                EnvironmentScope::System,
                "SYSTEM_VALUE",
                "two",
                EnvironmentValueType::String,
            ),
        ];
        assert_error_contains(
            serialize_export(TransferFileFormat::DotEnv, &mixed),
            "scope",
        );

        let directory = TempDirectory::new("dotenv-scope");
        let request = ExportFileRequest {
            path: directory.path().join("variables.env"),
            format: TransferFileFormat::DotEnv,
            scope: None,
        };
        assert_error_contains(write_export_file(&request, &mixed[..1]), "scope");
    }

    #[test]
    fn registry_accepts_utf16_and_utf8_boms_and_exact_environment_keys() {
        let text = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Environment]\r\n\"JAVA_HOME\"=\"C:\\\\Java\\\\jdk\"\r\n";
        let expected = variable(
            EnvironmentScope::User,
            "JAVA_HOME",
            r"C:\Java\jdk",
            EnvironmentValueType::String,
        );

        let utf16 = encode_utf16le_bom(text);
        let mut utf8 = vec![0xef, 0xbb, 0xbf];
        utf8.extend_from_slice(text.as_bytes());

        assert_eq!(
            parse_import_bytes(TransferFileFormat::Registry, &utf16, None).unwrap(),
            vec![expected.clone()]
        );
        assert_eq!(
            parse_import_bytes(TransferFileFormat::Registry, &utf8, None).unwrap(),
            vec![expected]
        );
    }

    #[test]
    fn registry_expandable_hex_with_continuation_round_trips() {
        let text = concat!(
            "Windows Registry Editor Version 5.00\r\n\r\n",
            "[HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment]\r\n",
            "\"Path\"=hex(2):25,00,53,00,79,00,73,00,74,00,65,00,6d,00,52,00,6f,00,6f,00,\\\r\n",
            "  74,00,25,00,5c,00,62,00,69,00,6e,00,00,00\r\n"
        );

        let variables = parse_import_bytes(
            TransferFileFormat::Registry,
            &encode_utf16le_bom(text),
            None,
        )
        .unwrap();

        assert_eq!(
            variables,
            vec![variable(
                EnvironmentScope::System,
                "Path",
                r"%SystemRoot%\bin",
                EnvironmentValueType::ExpandableString,
            )]
        );
        let exported = serialize_export(TransferFileFormat::Registry, &variables).unwrap();
        assert_eq!(
            parse_import_bytes(TransferFileFormat::Registry, &exported, None).unwrap(),
            variables
        );
    }

    #[test]
    fn registry_rejects_unrelated_keys_unsupported_operations_and_bad_hex() {
        let header = "Windows Registry Editor Version 5.00\r\n\r\n";
        for (body, expected) in [
            ("[HKEY_CURRENT_USER\\Software\\Example]\r\n\"A\"=\"B\"\r\n", "key"),
            ("[HKEY_CURRENT_USER\\Environment]\r\n\"A\"=dword:00000001\r\n", "unsupported"),
            ("[HKEY_CURRENT_USER\\Environment]\r\n\"A\"=-\r\n", "unsupported"),
            ("[HKEY_CURRENT_USER\\Environment]\r\n\"A\"=hex(2):zz,00\r\n", "hex"),
            ("[HKEY_CURRENT_USER\\Environment]\r\n\"A\"=hex(2):00,d8,00,00\r\n", "UTF-16"),
        ] {
            assert_error_contains(
                parse_import_bytes(
                    TransferFileFormat::Registry,
                    &encode_utf16le_bom(&format!("{header}{body}")),
                    None,
                ),
                expected,
            );
        }
    }

    #[test]
    fn registry_rejects_same_scope_case_insensitive_duplicates() {
        let text = concat!(
            "Windows Registry Editor Version 5.00\r\n\r\n",
            "[HKEY_CURRENT_USER\\Environment]\r\n",
            "\"Path\"=\"one\"\r\n",
            "\"PATH\"=\"two\"\r\n"
        );

        assert_error_contains(
            parse_import_bytes(
                TransferFileFormat::Registry,
                &encode_utf16le_bom(text),
                None,
            ),
            "duplicate",
        );
    }

    #[test]
    fn json_and_registry_mixed_scope_exports_are_stably_sorted() {
        let variables = vec![
            variable(
                EnvironmentScope::System,
                "Z_SYSTEM",
                "system",
                EnvironmentValueType::String,
            ),
            variable(
                EnvironmentScope::User,
                "A_USER",
                "user",
                EnvironmentValueType::String,
            ),
        ];
        let reversed = variables.iter().cloned().rev().collect::<Vec<_>>();

        for format in [TransferFileFormat::Json, TransferFileFormat::Registry] {
            let first = serialize_export(format, &variables).unwrap();
            let second = serialize_export(format, &reversed).unwrap();
            assert_eq!(first, second);
            assert_eq!(
                parse_import_bytes(format, &first, None).unwrap(),
                reversed
            );
        }

        let registry = decode_registry(
            &serialize_export(TransferFileFormat::Registry, &variables).unwrap(),
        );
        assert!(registry.starts_with("Windows Registry Editor Version 5.00"));
        assert!(registry.contains("[HKEY_CURRENT_USER\\Environment]"));
        assert!(registry.contains("[HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment]"));
    }

    #[test]
    fn import_file_rejects_files_larger_than_the_limit() {
        let directory = TempDirectory::new("size-limit");
        let path = directory.path().join("too-large.env");
        std::fs::write(&path, vec![b'A'; 10 * 1024 * 1024 + 1]).unwrap();
        let request = ImportFileRequest {
            path,
            format: TransferFileFormat::DotEnv,
            default_scope: Some(EnvironmentScope::User),
        };

        assert_error_contains(read_import_file(&request), "large");
    }

    #[test]
    fn atomic_export_replaces_destination_without_leaving_a_temp_file() {
        let directory = TempDirectory::new("atomic-success");
        let path = directory.path().join("variables.json");
        std::fs::write(&path, b"old contents").unwrap();
        let request = ExportFileRequest {
            path: path.clone(),
            format: TransferFileFormat::Json,
            scope: None,
        };
        let variables = vec![variable(
            EnvironmentScope::User,
            "JAVA_HOME",
            r"C:\Java",
            EnvironmentValueType::String,
        )];

        let summary = write_export_file(&request, &variables).unwrap();

        assert_eq!(summary.path, path);
        assert_eq!(summary.variable_count, 1);
        assert_ne!(std::fs::read(&path).unwrap(), b"old contents");
        assert!(
            std::fs::read_dir(directory.path())
                .unwrap()
                .all(|entry| entry.unwrap().path().extension().and_then(|value| value.to_str()) != Some("tmp"))
        );
    }

    #[test]
    fn serialization_failure_does_not_modify_an_existing_destination() {
        let directory = TempDirectory::new("atomic-failure");
        let path = directory.path().join("variables.env");
        std::fs::write(&path, b"keep me").unwrap();
        let request = ExportFileRequest {
            path: path.clone(),
            format: TransferFileFormat::DotEnv,
            scope: None,
        };
        let mixed = vec![
            variable(
                EnvironmentScope::User,
                "USER_VALUE",
                "one",
                EnvironmentValueType::String,
            ),
            variable(
                EnvironmentScope::System,
                "SYSTEM_VALUE",
                "two",
                EnvironmentValueType::String,
            ),
        ];

        assert!(write_export_file(&request, &mixed).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep me");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
