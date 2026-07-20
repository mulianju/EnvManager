# Environment Manager Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `fspec-dev-impelement-fe` or `executing-plans` to implement this plan task-by-task.

**Goal:** Replace the existing secret launcher with a Windows user/system environment variable manager.

**Architecture:** A Rust service owns validation, backups, registry operations, elevation checks, and `WM_SETTINGCHANGE`. React consumes typed Tauri commands and provides scope-specific variable and PATH editors.

**Tech Stack:** Tauri 2, Rust, `winreg`, `windows-sys`, React, TypeScript, Vite, Vitest.

---

### Task 1: Replace the domain model

**Files:**
- Delete: `src-tauri/src/domain/profile.rs`
- Delete: `src-tauri/src/domain/project.rs`
- Create: `src-tauri/src/domain/environment.rs`
- Modify: `src-tauri/src/domain/mod.rs`

**Steps:**
1. Write failing tests for variable names, case-insensitive identity, and PATH entry parsing.
2. Implement `EnvironmentScope`, `EnvironmentValueType`, `EnvironmentVariable`, and validators.
3. Run `cargo test domain::environment` and confirm all tests pass.

### Task 2: Implement the Windows registry store

**Files:**
- Create: `src-tauri/src/platform/mod.rs`
- Create: `src-tauri/src/platform/windows.rs`
- Modify: `src-tauri/Cargo.toml`

**Steps:**
1. Define an `EnvironmentStore` trait for list, set, and delete operations.
2. Implement HKCU/HKLM reads and typed writes with `winreg`.
3. Detect elevation and broadcast `WM_SETTINGCHANGE` with `windows-sys`.
4. Run `cargo check --all-targets` on the MSVC toolchain.

### Task 3: Add backups and mutation orchestration

**Files:**
- Create: `src-tauri/src/services/environment.rs`
- Create: `src-tauri/src/services/backup.rs`
- Test: `src-tauri/src/services/environment.rs`

**Steps:**
1. Write failing tests with an in-memory store.
2. Implement pre-mutation backups, mutation serialization, and restore reconciliation.
3. Ensure system mutations return `elevationRequired` before writes.
4. Run `cargo test services`.

### Task 4: Replace the Tauri API

**Files:**
- Rewrite: `src-tauri/src/api.rs`
- Modify: `src-tauri/src/lib.rs`
- Delete: `src-tauri/src/cli.rs`
- Delete: `src-tauri/src/bin/devsecrets.rs`

**Steps:**
1. Expose snapshot, save, delete, restore, and UAC restart commands.
2. Remove Bitwarden, profile, project, and sidecar commands.
3. Run `cargo test --all-targets` and `cargo check --all-targets`.

### Task 5: Rebuild the desktop UI

**Files:**
- Rewrite: `src/App.tsx`
- Rewrite: `src/App.css`
- Rewrite: `src/types.ts`
- Rewrite: `src/lib/api.ts`
- Create: `src/lib/environment.ts`
- Test: `src/lib/environment.test.ts`

**Steps:**
1. Build User, System, and Backups navigation with loading, empty, error, and elevation states.
2. Add variable editing and PATH ordered-entry controls.
3. Add backup restore confirmation and UAC restart action.
4. Run `pnpm test`, `pnpm build`, and browser screenshots at desktop and narrow widths.

### Task 6: Package and verify Windows

**Files:**
- Rewrite: `README.md`
- Modify: `src-tauri/tauri.conf.json`
- Delete: `scripts/prepare-sidecar.mjs`

**Steps:**
1. Document registry scopes, elevation, backups, and process refresh behavior.
2. Run all Rust and frontend checks without automatic formatting.
3. Build MSI and NSIS packages and inspect their contents.
4. Smoke-test the release application and record remaining manual elevated-write verification.
