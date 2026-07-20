#[cfg(windows)]
mod windows;

use crate::domain::environment::{EnvironmentScope, EnvironmentVariable, EnvironmentVariableInput};
use std::fmt;

#[derive(Debug)]
pub enum EnvironmentStoreError {
    AccessDenied,
    UnsupportedPlatform,
    OperationFailed(String),
}

impl fmt::Display for EnvironmentStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessDenied => formatter.write_str("Administrator permission is required."),
            Self::UnsupportedPlatform => {
                formatter.write_str("Environment registry management is only available on Windows.")
            }
            Self::OperationFailed(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for EnvironmentStoreError {}

pub trait EnvironmentStore: Send + Sync {
    fn list(
        &self,
        scope: EnvironmentScope,
    ) -> Result<Vec<EnvironmentVariable>, EnvironmentStoreError>;

    fn set(&self, input: &EnvironmentVariableInput) -> Result<(), EnvironmentStoreError>;

    fn delete(&self, scope: EnvironmentScope, name: &str) -> Result<(), EnvironmentStoreError>;

    fn is_elevated(&self) -> bool;

    fn broadcast_change(&self) -> Result<(), EnvironmentStoreError>;
}

pub fn system_store() -> Box<dyn EnvironmentStore> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsEnvironmentStore)
    }

    #[cfg(not(windows))]
    {
        Box::new(UnsupportedEnvironmentStore)
    }
}

pub fn restart_as_administrator() -> Result<(), EnvironmentStoreError> {
    #[cfg(windows)]
    {
        windows::restart_as_administrator()
    }

    #[cfg(not(windows))]
    {
        Err(EnvironmentStoreError::UnsupportedPlatform)
    }
}

pub fn launch_powershell(environment: &[(String, String)]) -> Result<(), EnvironmentStoreError> {
    #[cfg(windows)]
    {
        windows::launch_powershell(environment)
    }

    #[cfg(not(windows))]
    {
        let _ = environment;
        Err(EnvironmentStoreError::UnsupportedPlatform)
    }
}

#[cfg(not(windows))]
struct UnsupportedEnvironmentStore;

#[cfg(not(windows))]
impl EnvironmentStore for UnsupportedEnvironmentStore {
    fn list(
        &self,
        _scope: EnvironmentScope,
    ) -> Result<Vec<EnvironmentVariable>, EnvironmentStoreError> {
        Err(EnvironmentStoreError::UnsupportedPlatform)
    }

    fn set(&self, _input: &EnvironmentVariableInput) -> Result<(), EnvironmentStoreError> {
        Err(EnvironmentStoreError::UnsupportedPlatform)
    }

    fn delete(&self, _scope: EnvironmentScope, _name: &str) -> Result<(), EnvironmentStoreError> {
        Err(EnvironmentStoreError::UnsupportedPlatform)
    }

    fn is_elevated(&self) -> bool {
        false
    }

    fn broadcast_change(&self) -> Result<(), EnvironmentStoreError> {
        Err(EnvironmentStoreError::UnsupportedPlatform)
    }
}
