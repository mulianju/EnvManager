use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnvironmentScope {
    User,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnvironmentValueType {
    String,
    ExpandableString,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: String,
    pub value_type: EnvironmentValueType,
    pub scope: EnvironmentScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariableInput {
    pub original_name: Option<String>,
    pub name: String,
    pub value: String,
    pub value_type: EnvironmentValueType,
    pub scope: EnvironmentScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferMode {
    Copy,
    Move,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferVariableInput {
    pub source_scope: EnvironmentScope,
    pub target_scope: EnvironmentScope,
    pub name: String,
    pub mode: TransferMode,
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentValidationError {
    EmptyName,
    InvalidName,
    InvalidValue,
}

impl fmt::Display for EnvironmentValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("Environment variable name is required."),
            Self::InvalidName => {
                formatter.write_str("Environment variable name cannot contain '=' or NUL.")
            }
            Self::InvalidValue => {
                formatter.write_str("Environment variable value cannot contain NUL.")
            }
        }
    }
}

impl std::error::Error for EnvironmentValidationError {}

impl EnvironmentVariableInput {
    pub fn validate(&self) -> Result<(), EnvironmentValidationError> {
        validate_variable_name(&self.name)?;
        validate_variable_value(&self.value)
    }
}

impl TransferVariableInput {
    pub fn validate(&self) -> Result<(), EnvironmentValidationError> {
        validate_variable_name(&self.name)
    }
}

pub fn validate_variable_name(name: &str) -> Result<(), EnvironmentValidationError> {
    if name.trim().is_empty() {
        return Err(EnvironmentValidationError::EmptyName);
    }
    if name.contains('=') || name.contains('\0') {
        return Err(EnvironmentValidationError::InvalidName);
    }
    Ok(())
}

pub fn validate_variable_value(value: &str) -> Result<(), EnvironmentValidationError> {
    if value.contains('\0') {
        return Err(EnvironmentValidationError::InvalidValue);
    }
    Ok(())
}

pub fn variable_names_equal(left: &str, right: &str) -> bool {
    compare_variable_names(left, right) == Ordering::Equal
}

#[cfg(windows)]
pub fn compare_variable_names(left: &str, right: &str) -> Ordering {
    use windows_sys::Win32::Globalization::{
        CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringOrdinal,
    };

    let left = left.encode_utf16().collect::<Vec<_>>();
    let right = right.encode_utf16().collect::<Vec<_>>();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return fallback_variable_name_order(
            &String::from_utf16_lossy(&left),
            &String::from_utf16_lossy(&right),
        );
    };
    match unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) } {
        CSTR_LESS_THAN => Ordering::Less,
        CSTR_EQUAL => Ordering::Equal,
        CSTR_GREATER_THAN => Ordering::Greater,
        _ => fallback_variable_name_order(
            &String::from_utf16_lossy(&left),
            &String::from_utf16_lossy(&right),
        ),
    }
}

#[cfg(not(windows))]
pub fn compare_variable_names(left: &str, right: &str) -> Ordering {
    fallback_variable_name_order(left, right)
}

fn fallback_variable_name_order(left: &str, right: &str) -> Ordering {
    left.to_uppercase()
        .cmp(&right.to_uppercase())
        .then_with(|| left.to_lowercase().cmp(&right.to_lowercase()))
        .then_with(|| left.cmp(right))
}

pub fn is_path_variable(name: &str) -> bool {
    variable_names_equal(name, "Path")
}

pub fn parse_path_entries(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn join_path_entries(entries: &[String]) -> String {
    entries
        .iter()
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>()
        .join(";")
}

pub fn duplicate_path_entry_indexes(entries: &[String]) -> Vec<usize> {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        groups
            .entry(normalize_path_entry(entry))
            .or_default()
            .push(index);
    }

    let mut duplicates = groups
        .into_values()
        .filter(|indexes| indexes.len() > 1)
        .flatten()
        .collect::<Vec<_>>();
    duplicates.sort_unstable();
    duplicates
}

pub(crate) fn normalize_path_entry(entry: &str) -> String {
    let mut normalized = entry
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .to_ascii_lowercase();
    while normalized.len() > 3 && normalized.ends_with('\\') {
        normalized.pop();
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_windows_environment_variable_names() {
        assert!(validate_variable_name("JAVA_HOME").is_ok());
        assert!(validate_variable_name("Path").is_ok());
        assert!(validate_variable_name("").is_err());
        assert!(validate_variable_name("A=B").is_err());
        assert!(validate_variable_name("A\0B").is_err());
    }

    #[test]
    fn compares_variable_names_case_insensitively() {
        assert!(variable_names_equal("Path", "PATH"));
        assert!(variable_names_equal("ÄPFEL", "äpfel"));
        // Windows Registry permits these ordinally distinct names to coexist.
        assert!(!variable_names_equal("Σ", "ς"));
        assert!(!variable_names_equal("JAVA_HOME", "JDK_HOME"));
    }

    #[test]
    fn sorts_variable_names_stably_with_windows_identity() {
        let mut names = vec!["z", "ς", "Σ", "A", "a"];

        names.sort_by(|left, right| {
            compare_variable_names(left, right).then_with(|| left.cmp(right))
        });

        assert_eq!(names, vec!["A", "a", "z", "Σ", "ς"]);
    }

    #[test]
    fn parses_and_joins_path_entries() {
        let entries = parse_path_entries(r" C:\Tools ;%JAVA_HOME%\bin;;C:\Windows ");

        assert_eq!(
            entries,
            vec![r"C:\Tools", r"%JAVA_HOME%\bin", r"C:\Windows"]
        );
        assert_eq!(
            join_path_entries(&entries),
            r"C:\Tools;%JAVA_HOME%\bin;C:\Windows"
        );
    }

    #[test]
    fn identifies_duplicate_path_entries() {
        let entries = vec![
            r"C:\Tools".to_owned(),
            r"c:\tools\".to_owned(),
            r"C:\Windows".to_owned(),
        ];

        assert_eq!(duplicate_path_entry_indexes(&entries), vec![0, 1]);
    }
}
