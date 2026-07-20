use crate::domain::environment::{
    EnvironmentScope, EnvironmentValueType, EnvironmentVariable, compare_variable_names,
    validate_variable_name, validate_variable_value, variable_names_equal,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_IMPORT_SIZE: u64 = 10 * 1024 * 1024;
const JSON_SCHEMA_VERSION: u32 = 1;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferFileFormat {
    Json,
    DotEnv,
    Registry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportConflictStrategy {
    SkipExisting,
    Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportAction {
    Create,
    Update,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFileRequest {
    pub path: PathBuf,
    pub format: TransferFileFormat,
    pub default_scope: Option<EnvironmentScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFileRequest {
    pub path: PathBuf,
    pub format: TransferFileFormat,
    pub scope: Option<EnvironmentScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewItem {
    pub variable: EnvironmentVariable,
    pub existing: Option<EnvironmentVariable>,
    pub action: ImportAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub token: String,
    pub environment_revision: String,
    pub items: Vec<ImportPreviewItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub path: PathBuf,
    pub variable_count: usize,
}

#[derive(Debug)]
pub enum TransferFileError {
    Io(io::Error),
    InvalidFormat {
        format: TransferFileFormat,
        line: Option<usize>,
        message: String,
    },
    FileTooLarge,
}

impl TransferFileError {
    fn format(format: TransferFileFormat, message: impl Into<String>) -> Self {
        Self::InvalidFormat {
            format,
            line: None,
            message: message.into(),
        }
    }

    fn line(format: TransferFileFormat, line: usize, message: impl Into<String>) -> Self {
        Self::InvalidFormat {
            format,
            line: Some(line),
            message: message.into(),
        }
    }
}

impl fmt::Display for TransferFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Transfer file operation failed: {error}"),
            Self::InvalidFormat {
                format,
                line,
                message,
            } => match line {
                Some(line) => write!(
                    formatter,
                    "Invalid {format:?} file at line {line}: {message}"
                ),
                None => write!(formatter, "Invalid {format:?} file: {message}"),
            },
            Self::FileTooLarge => {
                formatter.write_str("Transfer file is too large (maximum 10 MiB).")
            }
        }
    }
}

impl std::error::Error for TransferFileError {}

impl From<io::Error> for TransferFileError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonDocument {
    schema_version: u32,
    variables: Vec<EnvironmentVariable>,
}

pub fn parse_import_bytes(
    format: TransferFileFormat,
    bytes: &[u8],
    default_scope: Option<EnvironmentScope>,
) -> Result<Vec<EnvironmentVariable>, TransferFileError> {
    let variables = match format {
        TransferFileFormat::Json => parse_json(strip_utf8_bom(bytes))?,
        TransferFileFormat::DotEnv => parse_dotenv(
            strip_utf8_bom(bytes),
            default_scope
                .ok_or_else(|| TransferFileError::format(format, "A default scope is required."))?,
        )?,
        TransferFileFormat::Registry => parse_registry(bytes)?,
    };
    validate_variables(format, &variables)?;
    Ok(variables)
}

pub fn serialize_export(
    format: TransferFileFormat,
    variables: &[EnvironmentVariable],
) -> Result<Vec<u8>, TransferFileError> {
    validate_variables(format, variables)?;
    let variables = sorted_variables(variables);
    match format {
        TransferFileFormat::Json => serde_json::to_vec_pretty(&JsonDocument {
            schema_version: JSON_SCHEMA_VERSION,
            variables,
        })
        .map_err(|error| TransferFileError::format(format, error.to_string())),
        TransferFileFormat::DotEnv => serialize_dotenv(&variables),
        TransferFileFormat::Registry => Ok(serialize_registry(&variables)),
    }
}

pub fn read_import_file(
    request: &ImportFileRequest,
) -> Result<Vec<EnvironmentVariable>, TransferFileError> {
    let file = File::open(&request.path)?;
    if file.metadata()?.len() > MAX_IMPORT_SIZE {
        return Err(TransferFileError::FileTooLarge);
    }
    let bytes = read_bounded(file)?;
    parse_import_bytes(request.format, &bytes, request.default_scope)
}

pub fn import_token(variables: &[EnvironmentVariable]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"env-manager-import-v1");
    for variable in sorted_variables(variables) {
        hasher.update([scope_order(variable.scope)]);
        hash_token_field(&mut hasher, variable.name.as_bytes());
        hasher.update([value_type_order(variable.value_type)]);
        hash_token_field(&mut hasher, variable.value.as_bytes());
    }
    let digest = hasher.finalize();
    let mut token = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut token, "{byte:02x}").expect("writing to String cannot fail");
    }
    token
}

fn hash_token_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn read_bounded(reader: impl Read) -> Result<Vec<u8>, TransferFileError> {
    let mut bytes = Vec::new();
    reader.take(MAX_IMPORT_SIZE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_IMPORT_SIZE {
        return Err(TransferFileError::FileTooLarge);
    }
    Ok(bytes)
}

fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes)
}

pub fn write_export_file(
    request: &ExportFileRequest,
    variables: &[EnvironmentVariable],
) -> Result<ExportSummary, TransferFileError> {
    if request.format == TransferFileFormat::DotEnv {
        let scope = request.scope.ok_or_else(|| {
            TransferFileError::format(request.format, "An explicit scope is required.")
        })?;
        if variables.iter().any(|variable| variable.scope != scope) {
            return Err(TransferFileError::format(
                request.format,
                "All variables must match the requested scope.",
            ));
        }
    }
    let bytes = serialize_export(request.format, variables)?;
    write_bytes_atomically(&request.path, &bytes)?;
    Ok(ExportSummary {
        path: request.path.clone(),
        variable_count: variables.len(),
    })
}

fn parse_json(bytes: &[u8]) -> Result<Vec<EnvironmentVariable>, TransferFileError> {
    let document: JsonDocument = serde_json::from_slice(bytes).map_err(|error| {
        TransferFileError::line(TransferFileFormat::Json, error.line(), error.to_string())
    })?;
    if document.schema_version != JSON_SCHEMA_VERSION {
        return Err(TransferFileError::format(
            TransferFileFormat::Json,
            format!(
                "Schema version {} is not supported.",
                document.schema_version
            ),
        ));
    }
    Ok(document.variables)
}

fn parse_dotenv(
    bytes: &[u8],
    scope: EnvironmentScope,
) -> Result<Vec<EnvironmentVariable>, TransferFileError> {
    let mut variables = Vec::new();
    let mut names = Vec::<String>::new();
    for (index, raw_line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let line_number = index + 1;
        let raw_line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        let line = std::str::from_utf8(raw_line).map_err(|_| {
            TransferFileError::line(TransferFileFormat::DotEnv, line_number, "Invalid UTF-8.")
        })?;
        let mut text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        if let Some(rest) = text
            .strip_prefix("export")
            .filter(|rest| rest.starts_with(char::is_whitespace))
        {
            text = rest.trim_start();
        }
        let (name, raw_value) = text.split_once('=').ok_or_else(|| {
            TransferFileError::line(
                TransferFileFormat::DotEnv,
                line_number,
                "Expected KEY=VALUE.",
            )
        })?;
        if !is_dotenv_name(name) {
            return Err(TransferFileError::line(
                TransferFileFormat::DotEnv,
                line_number,
                "Invalid variable name.",
            ));
        }
        if names
            .iter()
            .any(|existing| variable_names_equal(existing, name))
        {
            return Err(TransferFileError::line(
                TransferFileFormat::DotEnv,
                line_number,
                "Duplicate variable name.",
            ));
        }
        names.push(name.to_owned());
        let value = parse_dotenv_value(raw_value, line_number)?;
        variables.push(EnvironmentVariable {
            name: name.to_owned(),
            value,
            value_type: EnvironmentValueType::String,
            scope,
        });
    }
    Ok(variables)
}

fn is_dotenv_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn parse_dotenv_value(raw: &str, line: usize) -> Result<String, TransferFileError> {
    let value = raw.trim_start();
    if let Some(rest) = value.strip_prefix('\'') {
        let end = rest.find('\'').ok_or_else(|| {
            TransferFileError::line(TransferFileFormat::DotEnv, line, "Unclosed single quote.")
        })?;
        ensure_dotenv_trailer(&rest[end + 1..], line)?;
        return Ok(rest[..end].to_owned());
    }
    if let Some(rest) = value.strip_prefix('"') {
        let mut result = String::new();
        let mut escaped = false;
        for (index, character) in rest.char_indices() {
            if escaped {
                result.push(match character {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    '"' => '"',
                    _ => {
                        return Err(TransferFileError::line(
                            TransferFileFormat::DotEnv,
                            line,
                            "Unsupported escape sequence.",
                        ));
                    }
                });
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                ensure_dotenv_trailer(&rest[index + character.len_utf8()..], line)?;
                return Ok(result);
            } else {
                result.push(character);
            }
        }
        return Err(TransferFileError::line(
            TransferFileFormat::DotEnv,
            line,
            "Unclosed double quote.",
        ));
    }
    let comment = value
        .char_indices()
        .find(|(index, character)| {
            *character == '#' && (*index == 0 || value[..*index].ends_with(char::is_whitespace))
        })
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    Ok(value[..comment].trim_end().to_owned())
}

fn ensure_dotenv_trailer(trailer: &str, line: usize) -> Result<(), TransferFileError> {
    let trailer = trailer.trim();
    if trailer.is_empty() || trailer.starts_with('#') {
        Ok(())
    } else {
        Err(TransferFileError::line(
            TransferFileFormat::DotEnv,
            line,
            "Unexpected content after quoted value.",
        ))
    }
}

fn serialize_dotenv(variables: &[EnvironmentVariable]) -> Result<Vec<u8>, TransferFileError> {
    if variables
        .first()
        .is_some_and(|first| variables.iter().any(|item| item.scope != first.scope))
    {
        return Err(TransferFileError::format(
            TransferFileFormat::DotEnv,
            "DotEnv export supports one scope at a time.",
        ));
    }
    let mut output = String::new();
    for variable in variables {
        if !is_dotenv_name(&variable.name) {
            return Err(TransferFileError::format(
                TransferFileFormat::DotEnv,
                "Invalid DotEnv variable name.",
            ));
        }
        output.push_str(&variable.name);
        output.push_str("=\"");
        for character in variable.value.chars() {
            match character {
                '\n' => output.push_str("\\n"),
                '\r' => output.push_str("\\r"),
                '\t' => output.push_str("\\t"),
                '\\' => output.push_str("\\\\"),
                '"' => output.push_str("\\\""),
                other => output.push(other),
            }
        }
        output.push_str("\"\n");
    }
    Ok(output.into_bytes())
}

fn decode_registry(bytes: &[u8]) -> Result<String, TransferFileError> {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let body = &bytes[2..];
        if body.len() % 2 != 0 {
            return Err(TransferFileError::format(
                TransferFileFormat::Registry,
                "Malformed UTF-16LE data.",
            ));
        }
        let words = body
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&words).map_err(|_| {
            TransferFileError::format(TransferFileFormat::Registry, "Invalid UTF-16LE data.")
        })
    } else {
        let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        String::from_utf8(body.to_vec()).map_err(|_| {
            TransferFileError::format(TransferFileFormat::Registry, "Invalid UTF-8 data.")
        })
    }
}

fn parse_registry(bytes: &[u8]) -> Result<Vec<EnvironmentVariable>, TransferFileError> {
    let text = decode_registry(bytes)?;
    let physical = text.lines().collect::<Vec<_>>();
    if physical.first().copied() != Some("Windows Registry Editor Version 5.00") {
        return Err(TransferFileError::line(
            TransferFileFormat::Registry,
            1,
            "Missing standard registry header.",
        ));
    }
    let lines = registry_logical_lines(&physical)?;

    let mut scope = None;
    let mut variables = Vec::new();
    let mut names = Vec::<(EnvironmentScope, String)>::new();
    for (line_number, raw) in lines {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            scope = Some(match line {
                "[HKEY_CURRENT_USER\\Environment]" => EnvironmentScope::User,
                "[HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment]" => {
                    EnvironmentScope::System
                }
                _ => {
                    return Err(TransferFileError::line(
                        TransferFileFormat::Registry,
                        line_number,
                        "Unrelated or unsupported registry key.",
                    ));
                }
            });
            continue;
        }
        let current_scope = scope.ok_or_else(|| {
            TransferFileError::line(
                TransferFileFormat::Registry,
                line_number,
                "Registry value appears before a supported key.",
            )
        })?;
        let (name, data) = parse_registry_assignment(line, line_number)?;
        if names.iter().any(|(scope, existing)| {
            *scope == current_scope && variable_names_equal(existing, &name)
        }) {
            return Err(TransferFileError::line(
                TransferFileFormat::Registry,
                line_number,
                "Duplicate variable name.",
            ));
        }
        names.push((current_scope, name.clone()));
        let (value, value_type) = if let Some(quoted) = data.strip_prefix('"') {
            (
                parse_registry_quoted(quoted, line_number)?,
                EnvironmentValueType::String,
            )
        } else if let Some(hex) = data.strip_prefix("hex(2):") {
            (
                parse_expandable_hex(hex, line_number)?,
                EnvironmentValueType::ExpandableString,
            )
        } else {
            return Err(TransferFileError::line(
                TransferFileFormat::Registry,
                line_number,
                "Unsupported registry value operation or type.",
            ));
        };
        variables.push(EnvironmentVariable {
            name,
            value,
            value_type,
            scope: current_scope,
        });
    }
    Ok(variables)
}

fn registry_logical_lines(physical: &[&str]) -> Result<Vec<(usize, String)>, TransferFileError> {
    let mut lines = Vec::new();
    let mut index = 1;
    while index < physical.len() {
        let line_number = index + 1;
        let line = physical[index];
        if registry_hex_payload(line.trim()).is_none() {
            lines.push((line_number, line.to_owned()));
            index += 1;
            continue;
        }

        let (mut logical_line, mut continued) =
            consume_hex_fragment(line.trim_end(), line_number, false)?;
        while continued {
            index += 1;
            if index >= physical.len() {
                return Err(TransferFileError::line(
                    TransferFileFormat::Registry,
                    line_number,
                    "Unfinished hex(2) continuation.",
                ));
            }
            let continuation_line = index + 1;
            let (fragment, has_next) =
                consume_hex_fragment(physical[index].trim(), continuation_line, true)?;
            logical_line.push_str(fragment.trim());
            continued = has_next;
        }
        lines.push((line_number, logical_line));
        index += 1;
    }
    Ok(lines)
}

fn registry_hex_payload(line: &str) -> Option<&str> {
    if !line.starts_with('"') {
        return None;
    }
    let mut escaped = false;
    for (offset, character) in line[1..].char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            let rest = &line[offset + 2..];
            return rest.strip_prefix("=hex(2):");
        }
    }
    None
}

fn consume_hex_fragment(
    line: &str,
    line_number: usize,
    continuation_only: bool,
) -> Result<(String, bool), TransferFileError> {
    let trimmed = line.trim_end();
    let (without_marker, continued) = match trimmed.strip_suffix('\\') {
        Some(without_marker) => {
            if without_marker.trim_end().ends_with('\\') {
                return Err(TransferFileError::line(
                    TransferFileFormat::Registry,
                    line_number,
                    "Multiple hex(2) continuation markers are not allowed.",
                ));
            }
            (without_marker, true)
        }
        None => (trimmed, false),
    };
    let fragment = if continuation_only {
        without_marker
    } else {
        registry_hex_payload(without_marker.trim()).ok_or_else(|| {
            TransferFileError::line(
                TransferFileFormat::Registry,
                line_number,
                "Malformed hex(2) assignment.",
            )
        })?
    };
    if !is_valid_registry_hex_fragment(fragment) {
        return Err(TransferFileError::line(
            TransferFileFormat::Registry,
            line_number,
            "Invalid character in hex(2) continuation.",
        ));
    }
    Ok((without_marker.to_owned(), continued))
}

fn is_valid_registry_hex_fragment(fragment: &str) -> bool {
    let fragment = fragment.trim();
    if fragment.is_empty() {
        return false;
    }
    let mut bytes = fragment.split(',').peekable();
    while let Some(byte) = bytes.next() {
        let byte = byte.trim();
        if byte.is_empty() {
            return fragment.ends_with(',') && bytes.peek().is_none();
        }
        if byte.len() != 2 || !byte.bytes().all(|character| character.is_ascii_hexdigit()) {
            return false;
        }
    }
    true
}

fn parse_registry_assignment(
    line: &str,
    line_number: usize,
) -> Result<(String, &str), TransferFileError> {
    if !line.starts_with('"') {
        return Err(TransferFileError::line(
            TransferFileFormat::Registry,
            line_number,
            "Malformed registry value name.",
        ));
    }
    let mut escaped = false;
    for (index, character) in line[1..].char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            let end = index + 1;
            let encoded_name = &line[1..end];
            let rest = &line[end + 1..];
            let data = rest.strip_prefix('=').ok_or_else(|| {
                TransferFileError::line(
                    TransferFileFormat::Registry,
                    line_number,
                    "Malformed registry assignment.",
                )
            })?;
            return Ok((parse_registry_escapes(encoded_name, line_number)?, data));
        }
    }
    Err(TransferFileError::line(
        TransferFileFormat::Registry,
        line_number,
        "Unclosed registry value name.",
    ))
}

fn parse_registry_quoted(value: &str, line: usize) -> Result<String, TransferFileError> {
    if !value.ends_with('"') {
        return Err(TransferFileError::line(
            TransferFileFormat::Registry,
            line,
            "Unclosed registry string value.",
        ));
    }
    parse_registry_escapes(&value[..value.len() - 1], line)
}

fn parse_registry_escapes(value: &str, line: usize) -> Result<String, TransferFileError> {
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            if character == '"' {
                return Err(TransferFileError::line(
                    TransferFileFormat::Registry,
                    line,
                    "Unescaped quote in registry string.",
                ));
            }
            result.push(character);
            continue;
        }
        match chars.next() {
            Some('\\') => result.push('\\'),
            Some('"') => result.push('"'),
            _ => {
                return Err(TransferFileError::line(
                    TransferFileFormat::Registry,
                    line,
                    "Unsupported registry escape.",
                ));
            }
        }
    }
    Ok(result)
}

fn parse_expandable_hex(hex: &str, line: usize) -> Result<String, TransferFileError> {
    let bytes = if hex.trim().is_empty() {
        Vec::new()
    } else {
        hex.split(',')
            .map(|part| {
                let part = part.trim();
                if part.len() != 2 || !part.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(TransferFileError::line(
                        TransferFileFormat::Registry,
                        line,
                        "Malformed hex(2) byte.",
                    ));
                }
                u8::from_str_radix(part, 16).map_err(|_| {
                    TransferFileError::line(
                        TransferFileFormat::Registry,
                        line,
                        "Malformed hex(2) byte.",
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    if bytes.len() < 2 || bytes.len() % 2 != 0 || !bytes.ends_with(&[0, 0]) {
        return Err(TransferFileError::line(
            TransferFileFormat::Registry,
            line,
            "hex(2) must contain terminal-NUL UTF-16LE data.",
        ));
    }
    let words = bytes[..bytes.len() - 2]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&words).map_err(|_| {
        TransferFileError::line(
            TransferFileFormat::Registry,
            line,
            "Invalid UTF-16LE hex(2) data.",
        )
    })
}

fn serialize_registry(variables: &[EnvironmentVariable]) -> Vec<u8> {
    let mut text = String::from("Windows Registry Editor Version 5.00\r\n");
    for scope in [EnvironmentScope::User, EnvironmentScope::System] {
        let scoped = variables
            .iter()
            .filter(|variable| variable.scope == scope)
            .collect::<Vec<_>>();
        if scoped.is_empty() {
            continue;
        }
        text.push_str("\r\n");
        text.push_str(match scope {
            EnvironmentScope::User => "[HKEY_CURRENT_USER\\Environment]\r\n",
            EnvironmentScope::System => "[HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment]\r\n",
        });
        for variable in scoped {
            text.push('"');
            text.push_str(&escape_registry_string(&variable.name));
            match variable.value_type {
                EnvironmentValueType::String => {
                    text.push_str("\"=\"");
                    text.push_str(&escape_registry_string(&variable.value));
                    text.push_str("\"\r\n");
                }
                EnvironmentValueType::ExpandableString => {
                    text.push_str("\"=hex(2):");
                    let mut bytes = variable
                        .value
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .flat_map(u16::to_le_bytes)
                        .peekable();
                    while let Some(byte) = bytes.next() {
                        use std::fmt::Write as _;
                        write!(&mut text, "{byte:02x}").expect("writing to String cannot fail");
                        if bytes.peek().is_some() {
                            text.push(',');
                        }
                    }
                    text.push_str("\r\n");
                }
            }
        }
    }
    let mut bytes = vec![0xff, 0xfe];
    for word in text.encode_utf16() {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn escape_registry_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn validate_variables(
    format: TransferFileFormat,
    variables: &[EnvironmentVariable],
) -> Result<(), TransferFileError> {
    for (index, variable) in variables.iter().enumerate() {
        validate_variable_name(&variable.name)
            .map_err(|_| TransferFileError::format(format, "Invalid environment variable name."))?;
        validate_variable_value(&variable.value).map_err(|_| {
            TransferFileError::format(format, "Invalid environment variable value.")
        })?;
        if variables[..index].iter().any(|existing| {
            existing.scope == variable.scope && variable_names_equal(&existing.name, &variable.name)
        }) {
            return Err(TransferFileError::format(
                format,
                "Duplicate variable name in the same scope.",
            ));
        }
    }
    Ok(())
}

fn sorted_variables(variables: &[EnvironmentVariable]) -> Vec<EnvironmentVariable> {
    let mut sorted = variables.to_vec();
    sorted.sort_by(|left, right| {
        scope_order(left.scope)
            .cmp(&scope_order(right.scope))
            .then_with(|| compare_variable_names(&left.name, &right.name))
            .then_with(|| left.name.cmp(&right.name))
    });
    sorted
}

fn scope_order(scope: EnvironmentScope) -> u8 {
    match scope {
        EnvironmentScope::User => 0,
        EnvironmentScope::System => 1,
    }
}

fn value_type_order(value_type: EnvironmentValueType) -> u8 {
    match value_type {
        EnvironmentValueType::String => 0,
        EnvironmentValueType::ExpandableString => 1,
    }
}

pub(crate) fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let base = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut temporary = None;
    for _ in 0..100 {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = directory.join(format!(
            ".{base}.{nanos}-{}-{counter}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (temporary_path, mut file) = temporary.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Could not allocate a unique temporary export file.",
        )
    })?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let success = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::environment::{EnvironmentScope, EnvironmentValueType, EnvironmentVariable};
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
            error
                .to_ascii_lowercase()
                .contains(&expected.to_ascii_lowercase()),
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
    fn json_and_dotenv_accept_utf8_boms() {
        let mut json = vec![0xef, 0xbb, 0xbf];
        json.extend_from_slice(
            br#"{"schemaVersion":1,"variables":[{"name":"VALUE","value":"json","valueType":"string","scope":"user"}]}"#,
        );
        assert_eq!(
            parse_import_bytes(TransferFileFormat::Json, &json, None).unwrap(),
            vec![variable(
                EnvironmentScope::User,
                "VALUE",
                "json",
                EnvironmentValueType::String,
            )]
        );

        let dotenv = b"\xef\xbb\xbfVALUE=dotenv\n";
        assert_eq!(
            parse_import_bytes(
                TransferFileFormat::DotEnv,
                dotenv,
                Some(EnvironmentScope::User),
            )
            .unwrap(),
            vec![variable(
                EnvironmentScope::User,
                "VALUE",
                "dotenv",
                EnvironmentValueType::String,
            )]
        );
    }

    #[test]
    fn json_rejects_unicode_case_duplicate_names() {
        let json = r#"{"schemaVersion":1,"variables":[{"name":"ÄPFEL","value":"one","valueType":"string","scope":"user"},{"name":"äpfel","value":"two","valueType":"string","scope":"user"}]}"#;

        assert_error_contains(
            parse_import_bytes(TransferFileFormat::Json, json.as_bytes(), None),
            "duplicate",
        );
    }

    #[test]
    fn json_allows_windows_ordinally_distinct_sigma_names_in_one_scope() {
        let json = r#"{"schemaVersion":1,"variables":[{"name":"Σ","value":"one","valueType":"string","scope":"user"},{"name":"ς","value":"two","valueType":"string","scope":"user"}]}"#;

        let variables =
            parse_import_bytes(TransferFileFormat::Json, json.as_bytes(), None).unwrap();

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
        assert_eq!(variables[3].value, "line\nnext\rreturn\ttab\\slash\"quote");
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
    fn dotenv_treats_export_as_a_prefix_only_when_followed_by_whitespace() {
        let variables = parse_import_bytes(
            TransferFileFormat::DotEnv,
            b"exportFOO=value\nexport BAR=other\n",
            Some(EnvironmentScope::User),
        )
        .unwrap();

        assert_eq!(variables[0].name, "exportFOO");
        assert_eq!(variables[0].value, "value");
        assert_eq!(variables[1].name, "BAR");
        assert_eq!(variables[1].value, "other");
    }

    #[test]
    fn import_tokens_are_semantic_order_independent_and_value_sensitive() {
        let first = parse_import_bytes(
            TransferFileFormat::DotEnv,
            b"# first comment\nB=two\nA=one\n",
            Some(EnvironmentScope::User),
        )
        .unwrap();
        let second = parse_import_bytes(
            TransferFileFormat::DotEnv,
            b"A=one\n# different comment\nB=two\n",
            Some(EnvironmentScope::User),
        )
        .unwrap();
        let changed = parse_import_bytes(
            TransferFileFormat::DotEnv,
            b"A=changed\nB=two\n",
            Some(EnvironmentScope::User),
        )
        .unwrap();

        assert_eq!(import_token(&first), import_token(&second));
        assert_ne!(import_token(&first), import_token(&changed));
        assert_eq!(import_token(&first).len(), 64);
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
    fn registry_rejects_multiline_string_values() {
        let text = concat!(
            "Windows Registry Editor Version 5.00\r\n\r\n",
            "[HKEY_CURRENT_USER\\Environment]\r\n",
            "\"A\"=\"foo\\\r\n",
            "bar\"\r\n"
        );

        assert_error_contains(
            parse_import_bytes(
                TransferFileFormat::Registry,
                &encode_utf16le_bom(text),
                None,
            ),
            "line 4",
        );
    }

    #[test]
    fn registry_rejects_repeated_or_cross_record_hex_continuations() {
        let repeated_marker = concat!(
            "Windows Registry Editor Version 5.00\r\n\r\n",
            "[HKEY_CURRENT_USER\\Environment]\r\n",
            "\"A\"=hex(2):41,00,\\\\\r\n",
            "00,00\r\n"
        );
        assert_error_contains(
            parse_import_bytes(
                TransferFileFormat::Registry,
                &encode_utf16le_bom(repeated_marker),
                None,
            ),
            "line 4",
        );

        let crossed_record = concat!(
            "Windows Registry Editor Version 5.00\r\n\r\n",
            "[HKEY_CURRENT_USER\\Environment]\r\n",
            "\"A\"=hex(2):41,00,\\\r\n",
            "\"B\"=\"value\"\r\n"
        );
        assert_error_contains(
            parse_import_bytes(
                TransferFileFormat::Registry,
                &encode_utf16le_bom(crossed_record),
                None,
            ),
            "line 5",
        );

        let non_hex_fragment = concat!(
            "Windows Registry Editor Version 5.00\r\n\r\n",
            "[HKEY_CURRENT_USER\\Environment]\r\n",
            "\"A\"=hex(2):41,00,\\\r\n",
            "dead\r\n"
        );
        assert_error_contains(
            parse_import_bytes(
                TransferFileFormat::Registry,
                &encode_utf16le_bom(non_hex_fragment),
                None,
            ),
            "line 5",
        );
    }

    #[test]
    fn registry_rejects_unrelated_keys_unsupported_operations_and_bad_hex() {
        let header = "Windows Registry Editor Version 5.00\r\n\r\n";
        for (body, expected) in [
            (
                "[HKEY_CURRENT_USER\\Software\\Example]\r\n\"A\"=\"B\"\r\n",
                "key",
            ),
            (
                "[HKEY_CURRENT_USER\\Environment]\r\n\"A\"=dword:00000001\r\n",
                "unsupported",
            ),
            (
                "[HKEY_CURRENT_USER\\Environment]\r\n\"A\"=-\r\n",
                "unsupported",
            ),
            (
                "[HKEY_CURRENT_USER\\Environment]\r\n\"A\"=hex(2):zz,00\r\n",
                "hex",
            ),
            (
                "[HKEY_CURRENT_USER\\Environment]\r\n\"A\"=hex(2):00,d8,00,00\r\n",
                "UTF-16",
            ),
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
    fn registry_rejects_unicode_case_duplicate_names() {
        let text = concat!(
            "Windows Registry Editor Version 5.00\r\n\r\n",
            "[HKEY_CURRENT_USER\\Environment]\r\n",
            "\"ÄPFEL\"=\"one\"\r\n",
            "\"äpfel\"=\"two\"\r\n"
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
    fn registry_allows_windows_ordinally_distinct_sigma_names_in_one_scope() {
        let text = concat!(
            "Windows Registry Editor Version 5.00\r\n\r\n",
            "[HKEY_CURRENT_USER\\Environment]\r\n",
            "\"Σ\"=\"one\"\r\n",
            "\"ς\"=\"two\"\r\n"
        );

        let variables = parse_import_bytes(
            TransferFileFormat::Registry,
            &encode_utf16le_bom(text),
            None,
        )
        .unwrap();

        assert_eq!(variables.len(), 2);
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
            assert_eq!(parse_import_bytes(format, &first, None).unwrap(), reversed);
        }

        let registry =
            decode_registry(&serialize_export(TransferFileFormat::Registry, &variables).unwrap());
        assert!(registry.starts_with("Windows Registry Editor Version 5.00"));
        assert!(registry.contains("[HKEY_CURRENT_USER\\Environment]"));
        assert!(registry.contains(
            "[HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment]"
        ));
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
    fn bounded_reader_stops_after_limit_plus_one_byte() {
        let mut reader = std::io::Cursor::new(vec![b'A'; MAX_IMPORT_SIZE as usize + 1024]);

        assert_error_contains(read_bounded(&mut reader), "large");
        assert_eq!(reader.position(), MAX_IMPORT_SIZE + 1);
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
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .and_then(|value| value.to_str())
                != Some("tmp")
        }));
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
