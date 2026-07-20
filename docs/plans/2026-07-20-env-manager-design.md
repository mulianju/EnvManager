# Environment Manager Design

## Goal

Build a Windows-first desktop application for reading and safely managing user and system environment variables. User variables are stored under `HKCU\Environment`; system variables are stored under `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`.

## Scope

- Search and inspect User and System variables independently.
- Create, edit, and delete `REG_SZ` and `REG_EXPAND_SZ` values.
- Edit `PATH` as ordered entries with duplicate and missing-path diagnostics.
- Require an elevated process for system mutations and offer a UAC restart action.
- Create a full scope backup before every mutation or restore.
- Restore a selected backup and broadcast `WM_SETTINGCHANGE` after mutations.

Bitwarden integration, runtime profiles, project bindings, and command environment injection are removed.

## Architecture

React renders the variable list, editor, PATH entry controls, and backup history. Typed Tauri commands call a Rust `EnvironmentService` that validates names and values, serializes mutations, creates JSON backups, delegates registry access to a platform store, and broadcasts environment changes.

The Windows store uses the registry as the source of truth. System writes are rejected before registry access when the current process is not elevated. Non-Windows builds preserve the shared types and return a clear unsupported-platform error until a platform-specific adapter is added.

## Safety

- Registry state is re-read after every mutation.
- Every write, delete, or restore first creates a timestamped backup of the target scope.
- Backups never contain application secrets beyond values already present in the environment registry.
- Restore requires explicit confirmation and itself creates a rollback backup.
- New processes observe changes after the standard Windows environment-change broadcast; already-running shells retain their existing process environment.

## Verification

- Unit-test validation, case-insensitive identity, PATH parsing, and backup round trips.
- Test mutations against an in-memory store without changing the developer machine.
- Exercise Windows registry reads without writing.
- Verify user-scope writes against an isolated temporary registry key through the adapter boundary.
- Build and inspect Windows MSI and NSIS packages.
