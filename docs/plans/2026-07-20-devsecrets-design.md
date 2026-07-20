# DevSecrets Design

## Goal

Build a local desktop application for developers to map Bitwarden secrets to named runtime profiles and inject them only into explicitly launched commands. Windows is the first verified platform; macOS support is preserved through platform adapters.

## Non-goals

- Replacing Bitwarden as a password manager or synchronisation service.
- Persisting secret values, Bitwarden passwords, or `BW_SESSION` locally.
- Editing system-wide environment variables.
- Reading an already-open terminal's environment or mutating its parent process.

## Architecture

The React renderer presents profiles, projects, and diagnostics. The Tauri Rust core owns file access and all `bw` invocations. It obtains Bitwarden items only at execution time, resolves configured fields to environment variables, and passes those variables exclusively to a child process.

The local configuration stores profile metadata, mappings, and project path bindings. It contains no secret values. Windows will encrypt that configuration with DPAPI. macOS will implement the same `SecretStore` adapter with Keychain while retaining the shared configuration schema and CLI protocol.

## Bitwarden Contract

DevSecrets invokes the locally installed Bitwarden CLI. The user authenticates, unlocks, and syncs Bitwarden separately. A profile mapping references a Bitwarden item ID and a field name; a field may be the item's `login.password`, `login.username`, or a custom field. Collection names are only used during setup and are resolved to IDs before saving.

## Command Contract

`devsecrets run [profile] -- <command...>` resolves a named profile or the project binding for the current directory, fetches only the required fields, and launches the command with those environment variables. A missing CLI, locked vault, missing item, or missing field produces a non-zero exit code before the target command starts.

`devsecrets shell <profile>` prints shell-specific sourceable commands. It never prints a secret unless the user has explicitly run the command in a terminal where they accept that exposure.

## Initial Verification

- Unit-test pure profile and project resolution code.
- Unit-test rejection of invalid or duplicate variable mappings.
- Exercise the Bitwarden command wrapper with fixture output.
- Manually validate Windows diagnostics when `bw` is missing and after a real Bitwarden CLI login.
- Validate `run` by launching a harmless command that asserts a mapped environment variable is present.

