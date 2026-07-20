# DevSecrets Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `fspec-dev-impelement-fe` or `executing-plans` to implement this plan task-by-task.

**Goal:** Build a Windows-first, macOS-compatible desktop tool that maps Bitwarden CLI fields into temporary command environments.

**Architecture:** Tauri Rust commands own Bitwarden process execution, profile validation, configuration storage, and child-process injection. React renders a compact profile management UI through typed command APIs, while a Rust CLI shares the same domain and runtime resolution logic.

**Tech Stack:** Tauri 2, Rust, React, TypeScript, Vite, Vitest, serde.

---

### Task 1: Scaffold the desktop workspace

**Files:**
- Create: `package.json`
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/tauri.conf.json`
- Create: `src/main.tsx`

**Step 1:** Generate the official Tauri React TypeScript template with pnpm.

**Step 2:** Start the Vite front end and confirm the generated page loads.

**Step 3:** Run `pnpm tauri dev` and confirm the Windows host can build the native shell.

### Task 2: Define secret-free configuration models

**Files:**
- Create: `src-tauri/src/domain/profile.rs`
- Create: `src-tauri/src/domain/project.rs`
- Create: `src-tauri/src/domain/mod.rs`
- Test: `src-tauri/src/domain/profile.rs`

**Step 1:** Write tests for valid environment names, duplicate mapping rejection, and longest project-path match.

**Step 2:** Implement serde models and pure validators.

**Step 3:** Run `cargo test domain` and confirm all cases pass.

### Task 3: Implement Bitwarden CLI diagnostics

**Files:**
- Create: `src-tauri/src/services/bitwarden.rs`
- Create: `src-tauri/src/services/mod.rs`
- Test: `src-tauri/src/services/bitwarden.rs`

**Step 1:** Write tests around parsing `bw status` JSON and user-facing failure categories.

**Step 2:** Implement a command adapter that resolves the executable path, reads status, and lists items without logging secret content.

**Step 3:** Run `cargo test bitwarden` and manually verify the missing-CLI state on Windows.

### Task 4: Store mappings and expose typed Tauri commands

**Files:**
- Create: `src-tauri/src/services/config.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/main.rs`
- Test: `src-tauri/src/services/config.rs`

**Step 1:** Write tests for round-tripping a secret-free configuration document.

**Step 2:** Implement an encrypted Windows storage adapter and a macOS Keychain adapter boundary.

**Step 3:** Add Tauri commands for diagnostics, profile CRUD, mapping validation, and project bindings.

**Step 4:** Run `cargo test` and `cargo check`.

### Task 5: Build the profile-focused UI

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.css`
- Create: `src/lib/api.ts`
- Create: `src/types.ts`

**Step 1:** Render Bitwarden diagnostics and profile list with static fixture state.

**Step 2:** Connect typed Tauri commands and add profile/mapping editing with validation states.

**Step 3:** Add project binding and CLI command copy controls.

**Step 4:** Run `pnpm lint` (read-only if configured), `pnpm test`, and visually inspect the Windows window.

### Task 6: Add the runtime CLI

**Files:**
- Create: `src-tauri/src/bin/devsecrets.rs`
- Modify: `src-tauri/Cargo.toml`
- Test: `src-tauri/src/bin/devsecrets.rs`

**Step 1:** Write command parsing tests for explicit profile and project auto-resolution.

**Step 2:** Implement `run` so secret values only exist in the spawned child-process environment.

**Step 3:** Implement `shell` for PowerShell, zsh, and bash with an explicit warning boundary.

**Step 4:** Run an integration test that starts a harmless child process and asserts it receives the expected fixture variable.

### Task 7: Package and verify Windows first

**Files:**
- Modify: `README.md`
- Create: `docs/bitwarden-setup.md`

**Step 1:** Document Bitwarden CLI installation, unlock flow, permissions, and recovery from missing/locked states.

**Step 2:** Build the Windows package with `pnpm tauri build`.

**Step 3:** Verify the installed app detects the real CLI, creates a profile, and launches a selected test command with injected variables.

