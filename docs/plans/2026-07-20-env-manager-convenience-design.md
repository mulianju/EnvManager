# EnvManager Convenience Enhancements Design

## Goal

Turn EnvManager into a fast daily-use tool without creating a second source of truth for environment values.

## Product principles

- Windows registry remains the only source of environment variable values.
- Convenience metadata may persist variable identity (`scope` and `name`), never copied values.
- Destructive cleanup and cross-scope moves always show impact and create restorable backups.
- System-scope mutations keep the existing UAC boundary.
- External changes never overwrite an open editor silently.

## Main workflows

### Variable browsing

The User and System views support double-click editing, a compact actions menu, copying the name, raw value, or PowerShell expression, pinning favorites, and copying or moving a variable to the other scope. A new Effective view merges both scopes case-insensitively, labels the winning source, and highlights variables shadowed by a same-name user value. Effective PATH is displayed as System PATH followed by User PATH.

### PATH editing

The PATH editor supports a native folder picker, multiline/semicolon paste, HTML drag-and-drop ordering, status filters, and explicit cleanup actions. Duplicate cleanup keeps the first normalized entry. Missing-path cleanup is never automatic because network drives and temporarily unavailable paths can be valid. An inline change preview shows additions, removals, and moved entries before saving.

### Undo and external refresh

Every mutation returns the backup IDs created for that operation. The success notice exposes Undo, which restores those backups in reverse operation order. The frontend polls a lightweight registry revision rather than repeatedly loading backups. If the registry changes while no editor is open, the snapshot refreshes automatically. If an editor is open, the app shows a non-destructive refresh notice.

### Import and export

Native dialogs select input and output paths. JSON preserves scope and registry type. `.env` imports into a user-selected scope as string values. `.reg` accepts only `HKCU\\Environment` and `HKLM\\...\\Environment`, rejecting unrelated keys. All imports produce a create/update/conflict preview before applying and use the same backup and permission rules as interactive edits.

### Latest-environment PowerShell

The backend builds an effective environment block from current System and User registry values, concatenates PATH in Windows order, expands `%NAME%` references with bounded recursion, and launches PowerShell with that explicit environment. This avoids inheriting the app process's stale environment after edits.

### Tray quick panel

Tauri creates a hidden compact `quick` window and a tray icon. Clicking the tray icon toggles the quick panel. It searches effective variables, shows pinned variables first, and copies values through the clipboard plugin. Tray menu commands show the main window, launch a fresh PowerShell, or quit. Settings persist only favorite keys under `%APPDATA%\\EnvManager\\settings.json`.

## Error and permission behavior

- Cross-scope copy reports destination conflicts before writing.
- Cross-scope move checks both permissions before mutation and rolls back destination changes if source deletion fails.
- Import parsing errors identify the line or unsupported registry construct.
- System import, move, restore, and edit return the existing `elevationRequired` API code.
- Clipboard, dialog, settings, and shell-launch failures surface as normal dismissible errors.

## Verification

- Rust unit tests cover effective merging, transfer rollback, import/export codecs, settings, revision calculation, and environment composition.
- TypeScript tests cover PATH bulk parsing, cleanup, change previews, effective rows, filtering, and copy formats.
- Existing live HKCU integration test remains opt-in and is run before release.
- Desktop and quick-panel workflows are exercised at desktop and narrow dimensions.
- MSI and NSIS are rebuilt; MSI contents and release startup are inspected.

## Release and validation boundaries

- `pnpm dev` is a browser-only preview backed by in-memory sample data. It does not read or write the Windows Registry and cannot validate native dialogs, clipboard permissions, tray behavior, the quick window, UAC, or PowerShell launch.
- The desktop application is the validation target for User, System, Effective, PATH, import/export, favorites, Undo, external revision handling, tray, and QuickPanel workflows.
- Automated tests and the opt-in live HKCU test cover normal user-scope behavior. HKLM mutation and **Restart as administrator** remain manual checks because Windows must own the UAC consent boundary.
- Favorites are stored at `%APPDATA%\EnvManager\settings.json`; only `{scope,name}` identities are persisted. Mutation backups remain under `%APPDATA%\EnvManager\backups`.
- Version `0.2.0` is the first release containing the complete convenience workflow set described in this document.
