use super::{EnvironmentStore, EnvironmentStoreError};
use crate::domain::environment::{
    EnvironmentScope, EnvironmentValueType, EnvironmentVariable, EnvironmentVariableInput,
    normalize_variable_name, variable_names_equal,
};
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_QUERY};
use windows_sys::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SW_SHOWNORMAL, SendMessageTimeoutW, WM_SETTINGCHANGE,
};
use winreg::RegKey;
use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
};
use winreg::types::{FromRegValue, ToRegValue};

const USER_ENVIRONMENT_KEY: &str = "Environment";
const SYSTEM_ENVIRONMENT_KEY: &str =
    r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";
const MAX_ENVIRONMENT_BLOCK_CODE_UNITS: usize = 1_048_576;

pub struct WindowsEnvironmentStore;

impl EnvironmentStore for WindowsEnvironmentStore {
    fn list(
        &self,
        scope: EnvironmentScope,
    ) -> Result<Vec<EnvironmentVariable>, EnvironmentStoreError> {
        let key = open_key(scope, KEY_READ, "read environment variables")?;
        let mut variables = Vec::new();

        for item in key.enum_values() {
            let (name, raw_value) =
                item.map_err(|error| map_io_error(error, "enumerate environment variables"))?;
            let value_type = match raw_value.vtype {
                REG_SZ => EnvironmentValueType::String,
                REG_EXPAND_SZ => EnvironmentValueType::ExpandableString,
                _ => continue,
            };
            let value = String::from_reg_value(&raw_value)
                .map_err(|error| map_io_error(error, "decode an environment variable"))?;
            variables.push(EnvironmentVariable {
                name,
                value,
                value_type,
                scope,
            });
        }

        variables.sort_by_cached_key(|variable| normalize_variable_name(&variable.name));
        Ok(variables)
    }

    fn set(&self, input: &EnvironmentVariableInput) -> Result<(), EnvironmentStoreError> {
        let key = open_key(input.scope, KEY_SET_VALUE, "write an environment variable")?;
        let mut raw_value = input.value.to_reg_value();
        raw_value.vtype = match input.value_type {
            EnvironmentValueType::String => REG_SZ,
            EnvironmentValueType::ExpandableString => REG_EXPAND_SZ,
        };
        key.set_raw_value(&input.name, &raw_value)
            .map_err(|error| map_io_error(error, "write an environment variable"))?;

        if let Some(original_name) = &input.original_name
            && !variable_names_equal(original_name, &input.name)
        {
            delete_value_if_present(&key, original_name)?;
        }

        Ok(())
    }

    fn delete(&self, scope: EnvironmentScope, name: &str) -> Result<(), EnvironmentStoreError> {
        let key = open_key(scope, KEY_SET_VALUE, "delete an environment variable")?;
        delete_value_if_present(&key, name)
    }

    fn is_elevated(&self) -> bool {
        unsafe { IsUserAnAdmin() != 0 }
    }

    fn broadcast_change(&self) -> Result<(), EnvironmentStoreError> {
        let environment = to_wide("Environment");
        let mut result = 0usize;
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                environment.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                5_000,
                &mut result,
            );
        }
        Ok(())
    }
}

pub fn restart_as_administrator() -> Result<(), EnvironmentStoreError> {
    let executable = std::env::current_exe().map_err(|error| {
        EnvironmentStoreError::OperationFailed(format!(
            "Unable to locate the current executable: {error}"
        ))
    })?;
    let operation = to_wide("runas");
    let executable = path_to_wide(&executable);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            executable.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    if result as isize <= 32 {
        return Err(EnvironmentStoreError::OperationFailed(
            "Windows did not start an elevated process.".to_owned(),
        ));
    }
    Ok(())
}

pub fn launch_powershell() -> Result<(), EnvironmentStoreError> {
    let windows_directory = windows_directory()?;
    let executable = powershell_path_from_windows_directory(&windows_directory)?;
    if !executable.is_file() {
        return Err(EnvironmentStoreError::OperationFailed(format!(
            "Windows PowerShell was not found at {}.",
            executable.display()
        )));
    }
    let environment = fresh_windows_environment()?;

    Command::new(&executable)
        .arg("-NoLogo")
        .env_clear()
        .envs(environment)
        .creation_flags(0x00000010)
        .spawn()
        .map_err(|error| {
            EnvironmentStoreError::OperationFailed(format!(
                "Unable to launch {}: {error}",
                executable.display()
            ))
        })?;
    Ok(())
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct OwnedEnvironmentBlock(*mut core::ffi::c_void);

impl Drop for OwnedEnvironmentBlock {
    fn drop(&mut self) {
        unsafe {
            DestroyEnvironmentBlock(self.0);
        }
    }
}

fn fresh_windows_environment() -> Result<Vec<(OsString, OsString)>, EnvironmentStoreError> {
    let mut token = std::ptr::null_mut();
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut token,
        )
    } == 0
    {
        return Err(last_operation_error("open the current process token"));
    }
    let _token = OwnedHandle(token);

    let mut block = std::ptr::null_mut();
    if unsafe { CreateEnvironmentBlock(&mut block, token, 0) } == 0 {
        return Err(last_operation_error("create a fresh Windows environment"));
    }
    let block = OwnedEnvironmentBlock(block);
    let units = read_environment_block(block.0.cast::<u16>())?;
    parse_environment_block(&units)
}

fn read_environment_block(pointer: *const u16) -> Result<Vec<u16>, EnvironmentStoreError> {
    if pointer.is_null() {
        return Err(EnvironmentStoreError::OperationFailed(
            "Windows returned a null environment block.".to_owned(),
        ));
    }
    let mut units = Vec::new();
    for index in 0..MAX_ENVIRONMENT_BLOCK_CODE_UNITS {
        let unit = unsafe { *pointer.add(index) };
        units.push(unit);
        if unit == 0 && units.len() > 1 && units[units.len() - 2] == 0 {
            return Ok(units);
        }
    }
    Err(EnvironmentStoreError::OperationFailed(format!(
        "Windows environment block exceeded {MAX_ENVIRONMENT_BLOCK_CODE_UNITS} UTF-16 code units."
    )))
}

fn parse_environment_block(
    units: &[u16],
) -> Result<Vec<(OsString, OsString)>, EnvironmentStoreError> {
    if units.len() > MAX_ENVIRONMENT_BLOCK_CODE_UNITS {
        return Err(EnvironmentStoreError::OperationFailed(format!(
            "Windows environment block exceeded {MAX_ENVIRONMENT_BLOCK_CODE_UNITS} UTF-16 code units."
        )));
    }
    let block_end = units
        .windows(2)
        .position(|pair| pair == [0, 0])
        .ok_or_else(|| {
            EnvironmentStoreError::OperationFailed(
                "Windows environment block is missing its double-NUL terminator.".to_owned(),
            )
        })?;
    let mut environment = Vec::new();
    let mut cursor = 0;
    while cursor < block_end {
        let entry_end = units[cursor..=block_end]
            .iter()
            .position(|unit| *unit == 0)
            .map(|offset| cursor + offset)
            .ok_or_else(|| {
                EnvironmentStoreError::OperationFailed(
                    "Windows environment block contains an unterminated entry.".to_owned(),
                )
            })?;
        let entry = &units[cursor..entry_end];
        cursor = entry_end + 1;
        if entry.first() == Some(&('=' as u16)) {
            continue;
        }
        let separator = entry
            .iter()
            .position(|unit| *unit == '=' as u16)
            .ok_or_else(|| {
                EnvironmentStoreError::OperationFailed(
                    "Windows environment block contains an entry without '='.".to_owned(),
                )
            })?;
        environment.push((
            OsString::from_wide(&entry[..separator]),
            OsString::from_wide(&entry[separator + 1..]),
        ));
    }
    Ok(environment)
}

fn last_operation_error(action: &str) -> EnvironmentStoreError {
    EnvironmentStoreError::OperationFailed(format!(
        "Failed to {action}: {}",
        io::Error::last_os_error()
    ))
}

fn windows_directory() -> Result<PathBuf, EnvironmentStoreError> {
    let mut buffer = vec![0u16; 260];
    loop {
        let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            return Err(EnvironmentStoreError::OperationFailed(format!(
                "Unable to locate the Windows directory: {}",
                io::Error::last_os_error()
            )));
        }
        if length as usize >= buffer.len() {
            buffer.resize(length as usize + 1, 0);
            continue;
        }
        return Ok(PathBuf::from(std::ffi::OsString::from_wide(
            &buffer[..length as usize],
        )));
    }
}

fn powershell_path_from_windows_directory(
    windows_directory: &Path,
) -> Result<PathBuf, EnvironmentStoreError> {
    if !windows_directory.is_absolute() {
        return Err(EnvironmentStoreError::OperationFailed(format!(
            "Windows returned a non-absolute Windows directory: {}.",
            windows_directory.display()
        )));
    }
    Ok(windows_directory
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe"))
}

fn open_key(
    scope: EnvironmentScope,
    flags: u32,
    action: &str,
) -> Result<RegKey, EnvironmentStoreError> {
    let (root, path) = match scope {
        EnvironmentScope::User => (RegKey::predef(HKEY_CURRENT_USER), USER_ENVIRONMENT_KEY),
        EnvironmentScope::System => (RegKey::predef(HKEY_LOCAL_MACHINE), SYSTEM_ENVIRONMENT_KEY),
    };
    root.open_subkey_with_flags(path, flags)
        .map_err(|error| map_io_error(error, action))
}

fn delete_value_if_present(key: &RegKey, name: &str) -> Result<(), EnvironmentStoreError> {
    match key.delete_value(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(map_io_error(error, "delete an environment variable")),
    }
}

fn map_io_error(error: io::Error, action: &str) -> EnvironmentStoreError {
    if error.raw_os_error() == Some(5) {
        EnvironmentStoreError::AccessDenied
    } else {
        EnvironmentStoreError::OperationFailed(format!("Failed to {action}: {error}"))
    }
}

fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

fn path_to_wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment_block(entries: &[&str]) -> Vec<u16> {
        let mut block = entries
            .iter()
            .flat_map(|entry| OsStr::new(entry).encode_wide().chain(Some(0)))
            .collect::<Vec<_>>();
        block.push(0);
        block
    }

    #[test]
    fn reads_real_user_and_system_environment_keys_without_writing() {
        let store = WindowsEnvironmentStore;

        store.list(EnvironmentScope::User).unwrap();
        store.list(EnvironmentScope::System).unwrap();
    }

    #[test]
    fn rejects_a_relative_windows_directory_for_powershell() {
        let error = powershell_path_from_windows_directory(Path::new("Windows")).unwrap_err();

        assert!(error.to_string().contains("non-absolute"));
    }

    #[test]
    fn builds_an_absolute_powershell_path_from_the_windows_directory() {
        let executable = powershell_path_from_windows_directory(Path::new(r"C:\Windows")).unwrap();

        assert_eq!(
            executable,
            PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        );
        assert!(executable.is_absolute());
    }

    #[test]
    fn parses_environment_blocks_with_empty_values_and_pseudo_entries() {
        let block = environment_block(&["SystemRoot=C:\\Windows", "EMPTY=", "=C:=C:\\Work"]);

        let environment = parse_environment_block(&block).unwrap();

        assert_eq!(
            environment,
            vec![
                (OsString::from("SystemRoot"), OsString::from(r"C:\Windows")),
                (OsString::from("EMPTY"), OsString::new()),
            ]
        );
    }

    #[test]
    fn rejects_an_environment_block_without_a_double_nul_terminator() {
        let block = OsStr::new("SystemRoot=C:\\Windows")
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();

        let error = parse_environment_block(&block).unwrap_err();

        assert!(error.to_string().contains("double-NUL"));
    }

    #[test]
    fn creates_and_releases_a_fresh_windows_environment_block() {
        let environment = fresh_windows_environment().unwrap();

        assert!(environment.iter().any(|(name, _)| {
            name.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("SystemRoot")
        }));
        assert!(environment.iter().any(|(name, _)| {
            name.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("Path")
        }));
    }
}
