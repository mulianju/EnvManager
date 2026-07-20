# EnvManager Convenience Enhancements Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `fspec-dev-impelement-fe` or `executing-plans` to implement this plan task-by-task.

**Goal:** Add the full convenience workflow set to EnvManager while keeping the Windows registry as the sole source of variable values.

**Architecture:** Extend the Rust service with transactional mutation receipts, effective environment composition, revision checks, import/export codecs, launch support, and name-only favorites. React consumes typed commands in the main and quick-panel windows; Tauri owns native dialogs, clipboard permission, tray lifecycle, and window visibility.

**Tech Stack:** Tauri 2, Rust, `winreg`, `windows-sys`, React 19, TypeScript, Vite, Vitest, Tauri dialog and clipboard plugins.

---

### Task 1: Add frontend environment utilities

**Files:**
- Modify: `src/lib/environment.ts`
- Modify: `src/lib/environment.test.ts`
- Modify: `src/types.ts`

**Steps:**
1. Add failing tests for multiline PATH parsing, first-entry deduplication, status filtering, PATH change summaries, effective variable merging, and copy formats.
2. Run `pnpm test` and verify the new tests fail.
3. Add typed pure functions with case-insensitive Windows identity rules.
4. Run `pnpm test` and verify all utility tests pass.

### Task 2: Return mutation receipts and support atomic scope transfer

**Files:**
- Modify: `src-tauri/src/services/environment.rs`
- Modify: `src-tauri/src/services/backup.rs`
- Modify: `src-tauri/src/domain/environment.rs`

**Steps:**
1. Add in-memory-store tests for backup IDs, multi-backup undo, copy conflict, move, permission preflight, and destination rollback.
2. Introduce `MutationResult`, `TransferVariableInput`, and `TransferMode`.
3. Return created backup IDs from set/delete/restore/import operations.
4. Implement transfer under the existing service lock with preflight and rollback.
5. Run targeted Rust tests.

### Task 3: Add effective environment, revision, and PowerShell launch

**Files:**
- Modify: `src-tauri/src/services/environment.rs`
- Modify: `src-tauri/src/platform/mod.rs`
- Modify: `src-tauri/src/platform/windows.rs`

**Steps:**
1. Add tests for user-over-system precedence, combined PATH, bounded variable expansion, and stable revision hashing.
2. Add effective-variable metadata and a lightweight registry revision command.
3. Compose an explicit child environment and launch `powershell.exe` with current registry values.
4. Keep unsupported-platform behavior explicit.
5. Run Rust tests and `cargo check --all-targets`.

### Task 4: Implement import/export codecs and file operations

**Files:**
- Create: `src-tauri/src/services/transfer_file.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/services/environment.rs`

**Steps:**
1. Add fixtures/tests for JSON, `.env`, UTF-16 `.reg`, escaped values, invalid lines, and forbidden registry keys.
2. Implement structured JSON serialization with schema version and type/scope preservation.
3. Implement conservative `.env` parsing and deterministic export.
4. Implement the supported `.reg` subset for the two environment keys.
5. Add preview and apply operations; require explicit conflict strategy.
6. Run targeted Rust tests.

### Task 5: Persist favorites without values

**Files:**
- Create: `src-tauri/src/services/settings.rs`
- Modify: `src-tauri/src/services/mod.rs`

**Steps:**
1. Add round-trip, case-insensitive identity, corrupt-file, and traversal-independent tests.
2. Persist only `{scope,name}` keys under the application data directory.
3. Add list/toggle commands that reconcile missing variables at read time.
4. Run targeted Rust tests.

### Task 6: Extend Tauri API and desktop shell

**Files:**
- Modify: `src-tauri/src/api.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `package.json`

**Steps:**
1. Add typed commands for undo, transfer, revision, PowerShell, import/export, folder/file dialogs, and favorites.
2. Add official Tauri dialog and clipboard plugins with least-required permissions.
3. Create a hidden compact quick window and tray icon using Tauri core APIs.
4. Make tray click toggle the quick window; add Show, New PowerShell, and Quit menu actions.
5. Run Cargo check and TypeScript build.

### Task 7: Upgrade the variable and Effective views

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.css`
- Modify: `src/lib/api.ts`
- Modify: `src/types.ts`

**Steps:**
1. Add Effective navigation and source/conflict presentation.
2. Add double-click editing and an actions menu for copy, pin, copy-to-scope, and move-to-scope.
3. Add transactional confirmations for collisions and moves.
4. Add success notices with one-click Undo.
5. Add revision polling that defers refresh while an editor or confirmation is open.
6. Verify loading, empty, error, disabled, and narrow states.

### Task 8: Upgrade the PATH editor

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.css`
- Modify: `src/lib/api.ts`

**Steps:**
1. Add native folder selection and deduplicating insertion.
2. Add multiline paste and HTML drag-and-drop ordering.
3. Add All, Duplicate, and Missing filters plus explicit duplicate cleanup.
4. Add an inline additions/removals/order change preview.
5. Add focused component/pure-function tests and verify narrow layout.

### Task 9: Add import/export workflows

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.css`
- Modify: `src/lib/api.ts`
- Modify: `src/types.ts`

**Steps:**
1. Add an option menu for JSON, `.env`, and `.reg` import/export.
2. Present import scope choice where required and a create/update/conflict preview.
3. Require explicit confirmation before applying updates.
4. Refresh the snapshot and expose Undo after import.
5. Test parser errors, cancellation, permission failures, and empty files.

### Task 10: Build the tray quick panel

**Files:**
- Create: `src/QuickPanel.tsx`
- Modify: `src/main.tsx`
- Modify: `src/App.css`
- Modify: `src/lib/api.ts`

**Steps:**
1. Route `?mode=quick` to the compact panel.
2. Search effective variables with favorites first and sensitive values masked.
3. Add copy and reveal controls plus keyboard navigation.
4. Refresh on focus and registry revision changes.
5. Verify tray toggle, clipboard behavior, and compact dimensions.

### Task 11: Document and release-verify

**Files:**
- Modify: `README.md`
- Modify: `docs/plans/2026-07-20-env-manager-convenience-design.md`

**Steps:**
1. Document all workflows, format boundaries, settings location, undo semantics, and PowerShell behavior.
2. Run `pnpm test`, `pnpm build`, `cargo test --all-targets`, the ignored live HKCU test, `cargo check --all-targets`, and `cargo fmt --check` without auto-fix.
3. Exercise main-window and quick-panel flows at desktop and narrow widths.
4. Build MSI and NSIS, inspect MSI contents, and smoke-start the release executable.
5. Run final `git diff --check` and report the remaining manual HKLM/UAC verification boundary.
