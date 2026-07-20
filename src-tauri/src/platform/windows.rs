use super::{EnvironmentStore, EnvironmentStoreError};
use crate::domain::environment::{
    EnvironmentScope, EnvironmentValueType, EnvironmentVariable, EnvironmentVariableInput,
    variable_names_equal,
};
use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;
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

        variables.sort_by_cached_key(|variable| variable.name.to_ascii_lowercase());
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

pub fn launch_powershell(environment: &[(String, String)]) -> Result<(), EnvironmentStoreError> {
    let windows_directory = windows_directory()?;
    let executable = powershell_path_from_windows_directory(&windows_directory)?;
    if !executable.is_file() {
        return Err(EnvironmentStoreError::OperationFailed(format!(
            "Windows PowerShell was not found at {}.",
            executable.display()
        )));
    }

    Command::new(&executable)
        .arg("-NoLogo")
        .env_clear()
        .envs(environment.iter().cloned())
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
}
