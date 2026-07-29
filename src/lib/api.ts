import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  ApiError,
  CommandShimInput,
  CommandShimSnapshot,
  EnvironmentScope,
  EnvironmentSnapshot,
  EnvironmentVariable,
  EnvironmentVariableInput,
  ExportFileRequest,
  ExportSummary,
  FavoriteKey,
  ImportConflictStrategy,
  ImportFileRequest,
  ImportPreview,
  MutationResult,
  PathEntryStatus,
  TransferFileFormat,
  TransferVariableInput,
} from "../types";
import {
  duplicatePathEntryIndexes,
  mergeEffectiveVariables,
  normalizePathEntry,
  previewVariableNamesEqual,
} from "./environment";

const browserPreview = typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

let previewRevision = 0;
let previewFavorites: FavoriteKey[] = [];
let previewCommandShimId = 1;
let previewCommandShims: CommandShimSnapshot = {
  items: [
    {
      id: "preview-sharedev",
      commandName: "sharedev",
      executable: "C:\\Tools\\Node\\node.exe",
      fixedArguments: ["C:\\Tools\\sharedev\\dist\\sharedev.js"],
      shimPath: "C:\\Users\\Developer\\AppData\\Local\\EnvManager\\bin\\sharedev.cmd",
      status: "ready",
      statusMessage: null,
      createdAtMs: Date.now() - 86_400_000,
      updatedAtMs: Date.now() - 3_600_000,
    },
  ],
  managedDirectory: "C:\\Users\\Developer\\AppData\\Local\\EnvManager\\bin",
  pathReady: true,
};
let previewSnapshot: EnvironmentSnapshot = {
  userVariables: [
    {
      name: "Path",
      value: "%USERPROFILE%\\AppData\\Local\\Programs;%JAVA_HOME%\\bin",
      valueType: "expandableString",
      scope: "user",
    },
    {
      name: "JAVA_HOME",
      value: "C:\\Program Files\\Java\\jdk-21",
      valueType: "string",
      scope: "user",
    },
  ],
  systemVariables: [
    {
      name: "ComSpec",
      value: "%SystemRoot%\\system32\\cmd.exe",
      valueType: "expandableString",
      scope: "system",
    },
    {
      name: "Path",
      value: "%SystemRoot%\\system32;%SystemRoot%",
      valueType: "expandableString",
      scope: "system",
    },
    {
      name: "SystemRoot",
      value: "C:\\Windows",
      valueType: "string",
      scope: "system",
    },
  ],
  effectiveVariables: [],
  revision: previewRevisionValue(),
  isElevated: false,
  backups: [],
  backupDirectory: "%APPDATA%\\EnvManager\\backups",
};
const previewBackupValues = new Map<string, EnvironmentVariable[]>();
refreshPreviewDerivedState();

export async function getEnvironmentSnapshot(): Promise<EnvironmentSnapshot> {
  if (browserPreview) return structuredClone(previewSnapshot);
  return invoke<EnvironmentSnapshot>("get_environment_snapshot");
}

export async function getCommandShims(): Promise<CommandShimSnapshot> {
  if (browserPreview) return structuredClone(previewCommandShims);
  return invoke<CommandShimSnapshot>("get_command_shims");
}

export async function saveCommandShim(
  input: CommandShimInput,
): Promise<CommandShimSnapshot> {
  if (browserPreview) {
    const commandName = input.commandName.trim();
    if (!commandName) throw previewError("invalidCommandName", "A command name is required.");
    if (!input.executable) throw previewError("shimTargetMissing", "Select an executable.");
    const duplicate = previewCommandShims.items.find(
      (item) => item.id !== input.id && item.commandName.toLowerCase() === commandName.toLowerCase(),
    );
    if (duplicate) throw previewError("shimConflict", `${duplicate.commandName} already exists.`);
    const existing = input.id
      ? previewCommandShims.items.find((item) => item.id === input.id)
      : null;
    if (input.id && !existing) throw previewError("shimOperationFailed", "Command Shim was not found.");
    const now = Date.now();
    const next = {
      id: existing?.id ?? `preview-shim-${previewCommandShimId++}`,
      commandName,
      executable: input.executable,
      fixedArguments: [...input.fixedArguments],
      shimPath: `${previewCommandShims.managedDirectory}\\${commandName}.cmd`,
      status: "ready" as const,
      statusMessage: null,
      createdAtMs: existing?.createdAtMs ?? now,
      updatedAtMs: now,
    };
    previewCommandShims.items = previewCommandShims.items
      .filter((item) => item.id !== next.id)
      .concat(next)
      .sort((left, right) => left.commandName.localeCompare(right.commandName));
    previewCommandShims.pathReady = true;
    return structuredClone(previewCommandShims);
  }
  return invoke<CommandShimSnapshot>("save_command_shim", { input });
}

export async function deleteCommandShim(id: string): Promise<CommandShimSnapshot> {
  if (browserPreview) {
    if (!previewCommandShims.items.some((item) => item.id === id)) {
      throw previewError("shimOperationFailed", "Command Shim was not found.");
    }
    previewCommandShims.items = previewCommandShims.items.filter((item) => item.id !== id);
    return structuredClone(previewCommandShims);
  }
  return invoke<CommandShimSnapshot>("delete_command_shim", { id });
}

export async function saveEnvironmentVariable(
  input: EnvironmentVariableInput,
  expectedRevision: string,
): Promise<MutationResult> {
  if (browserPreview) {
    assertPreviewRevision(expectedRevision);
    assertPreviewPermission(input.scope);
    validatePreviewName(input.name);
    const variables = scopeVariables(input.scope);
    if (!input.originalName && variables.some((item) => equalNames(item.name, input.name))) {
      throw previewError("variableAlreadyExists", `${input.name} already exists.`);
    }
    const backupId = createPreviewBackup(input.scope, "beforeSet");
    const next: EnvironmentVariable = {
      name: input.name,
      value: input.value,
      valueType: input.valueType,
      scope: input.scope,
    };
    setScopeVariables(
      input.scope,
      variables
        .filter((item) => !equalNames(item.name, input.originalName ?? input.name))
        .concat(next)
        .sort((left, right) => left.name.localeCompare(right.name)),
    );
    return previewMutationResult([backupId]);
  }
  return invoke<MutationResult>("save_environment_variable", { input, expectedRevision });
}

export async function deleteEnvironmentVariable(
  scope: EnvironmentScope,
  name: string,
  expectedRevision: string,
): Promise<MutationResult> {
  if (browserPreview) {
    assertPreviewRevision(expectedRevision);
    assertPreviewPermission(scope);
    validatePreviewName(name);
    const variables = scopeVariables(scope);
    if (!variables.some((item) => equalNames(item.name, name))) {
      throw previewError("variableNotFound", `${name} was not found.`);
    }
    const backupId = createPreviewBackup(scope, "beforeDelete");
    setScopeVariables(
      scope,
      variables.filter((item) => !equalNames(item.name, name)),
    );
    return previewMutationResult([backupId]);
  }
  return invoke<MutationResult>("delete_environment_variable", {
    scope,
    name,
    expectedRevision,
  });
}

export async function restoreEnvironmentBackup(
  backupId: string,
  expectedRevision: string,
): Promise<MutationResult> {
  if (browserPreview) {
    assertPreviewRevision(expectedRevision);
    const backup = previewSnapshot.backups.find((item) => item.id === backupId);
    if (!backup) throw previewError("backupOperationFailed", "Backup was not found.");
    assertPreviewPermission(backup.scope);
    const variables = previewBackupValues.get(backupId);
    if (!variables) throw previewError("backupOperationFailed", "Backup contents were not found.");
    const rollbackId = createPreviewBackup(backup.scope, "beforeRestore");
    setScopeVariables(backup.scope, structuredClone(variables));
    return previewMutationResult([rollbackId]);
  }
  return invoke<MutationResult>("restore_environment_backup", {
    backupId,
    expectedRevision,
  });
}

export async function undoEnvironmentMutation(
  backupIds: string[],
  expectedRevision: string,
): Promise<MutationResult> {
  if (browserPreview) {
    assertPreviewRevision(expectedRevision);
    if (backupIds.length === 0) {
      throw previewError("invalidUndo", "At least one backup is required.");
    }
    const seenScopes = new Set<EnvironmentScope>();
    const backups = backupIds.map((backupId) => {
      const summary = previewSnapshot.backups.find((item) => item.id === backupId);
      const variables = previewBackupValues.get(backupId);
      if (!summary || !variables) {
        throw previewError("backupOperationFailed", "Backup was not found.");
      }
      if (seenScopes.has(summary.scope)) {
        throw previewError("invalidUndo", "Only one backup per scope can be restored.");
      }
      seenScopes.add(summary.scope);
      assertPreviewPermission(summary.scope);
      return { summary, variables };
    });
    const rollbackIds = backups.map(({ summary }) =>
      createPreviewBackup(summary.scope, "beforeUndo"),
    );
    backups.forEach(({ summary, variables }) => {
      setScopeVariables(summary.scope, structuredClone(variables));
    });
    return previewMutationResult(rollbackIds);
  }
  return invoke<MutationResult>("undo_environment_mutation", {
    backupIds,
    expectedRevision,
  });
}

export async function transferEnvironmentVariable(
  input: TransferVariableInput,
  expectedRevision: string,
): Promise<MutationResult> {
  if (browserPreview) {
    assertPreviewRevision(expectedRevision);
    if (input.sourceScope === input.targetScope) {
      throw previewError("invalidTransfer", "Source and target scopes must be different.");
    }
    assertPreviewPermission(input.targetScope);
    if (input.mode === "move") assertPreviewPermission(input.sourceScope);
    const sourceVariables = scopeVariables(input.sourceScope);
    const targetVariables = scopeVariables(input.targetScope);
    const source = sourceVariables.find((item) => equalNames(item.name, input.name));
    if (!source) throw previewError("variableNotFound", `${input.name} was not found.`);
    const destination = targetVariables.find((item) => equalNames(item.name, input.name));
    if (destination && !input.overwrite) {
      throw previewError("variableAlreadyExists", `${destination.name} already exists.`);
    }
    const backupIds = [createPreviewBackup(input.targetScope, "beforeTransfer")];
    if (input.mode === "move") {
      backupIds.push(createPreviewBackup(input.sourceScope, "beforeTransfer"));
    }
    setScopeVariables(
      input.targetScope,
      targetVariables
        .filter((item) => !equalNames(item.name, source.name))
        .concat({ ...source, scope: input.targetScope })
        .sort((left, right) => left.name.localeCompare(right.name)),
    );
    if (input.mode === "move") {
      setScopeVariables(
        input.sourceScope,
        sourceVariables.filter((item) => !equalNames(item.name, source.name)),
      );
    }
    return previewMutationResult(backupIds);
  }
  return invoke<MutationResult>("transfer_environment_variable", {
    input,
    expectedRevision,
  });
}

export async function getEnvironmentRevision(): Promise<string> {
  if (browserPreview) return previewSnapshot.revision;
  return invoke<string>("get_environment_revision");
}

export async function launchPowerShell(): Promise<void> {
  if (browserPreview) return;
  return invoke<void>("launch_powershell");
}

export async function previewEnvironmentImport(
  request: ImportFileRequest,
): Promise<ImportPreview> {
  if (browserPreview) {
    return {
      token: previewImportToken(request),
      environmentRevision: previewSnapshot.revision,
      items: [],
    };
  }
  return invoke<ImportPreview>("preview_environment_import", { request });
}

export async function applyEnvironmentImport(
  request: ImportFileRequest,
  strategy: ImportConflictStrategy,
  expectedToken: string,
  expectedRevision: string,
): Promise<MutationResult> {
  if (browserPreview) {
    if (
      expectedToken !== previewImportToken(request) ||
      expectedRevision !== previewSnapshot.revision
    ) {
      throw previewError("importPreviewChanged", "Import preview has changed.");
    }
    return { snapshot: structuredClone(previewSnapshot), undoBackupIds: [] };
  }
  return invoke<MutationResult>("apply_environment_import", {
    request,
    strategy,
    expectedToken,
    expectedRevision,
  });
}

export async function exportEnvironmentFile(
  request: ExportFileRequest,
): Promise<ExportSummary> {
  if (browserPreview) {
    const variableCount = request.scope
      ? scopeVariables(request.scope).length
      : previewSnapshot.userVariables.length + previewSnapshot.systemVariables.length;
    return { path: request.path, variableCount };
  }
  return invoke<ExportSummary>("export_environment_file", { request });
}

export async function getFavorites(): Promise<FavoriteKey[]> {
  if (browserPreview) {
    previewFavorites = reconcilePreviewFavorites(previewFavorites);
    return structuredClone(previewFavorites);
  }
  return invoke<FavoriteKey[]>("get_favorites");
}

export async function toggleFavorite(favorite: FavoriteKey): Promise<FavoriteKey[]> {
  if (browserPreview) {
    validatePreviewName(favorite.name);
    const variable = scopeVariables(favorite.scope).find((item) =>
      equalNames(item.name, favorite.name),
    );
    if (!variable) throw previewError("variableNotFound", `${favorite.name} was not found.`);
    const index = previewFavorites.findIndex(
      (item) => item.scope === favorite.scope && equalNames(item.name, variable.name),
    );
    if (index >= 0) previewFavorites.splice(index, 1);
    else previewFavorites.push({ scope: favorite.scope, name: variable.name });
    previewFavorites.sort(compareFavorites);
    return structuredClone(previewFavorites);
  }
  return invoke<FavoriteKey[]>("toggle_favorite", { favorite });
}

export async function pickEnvironmentFolder(): Promise<string | null> {
  if (browserPreview) return null;
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export async function pickCommandShimExecutable(): Promise<string | null> {
  if (browserPreview) return null;
  const selected = await open({
    multiple: false,
    filters: [{ name: "Executable files", extensions: ["exe", "com"] }],
  });
  return typeof selected === "string" ? selected : null;
}

export async function pickCommandShimArgument(): Promise<string | null> {
  if (browserPreview) return null;
  const selected = await open({ multiple: false });
  return typeof selected === "string" ? selected : null;
}

export async function pickImportFile(): Promise<string | null> {
  if (browserPreview) return null;
  const selected = await open({
    multiple: false,
    filters: [{ name: "Environment files", extensions: ["json", "env", "reg"] }],
  });
  return typeof selected === "string" ? selected : null;
}

export async function pickExportFile(
  format: TransferFileFormat,
  defaultName: string,
): Promise<string | null> {
  if (browserPreview) return null;
  const extension = { json: "json", dotEnv: "env", registry: "reg" }[format];
  return save({
    defaultPath: defaultName,
    filters: [{ name: "Environment file", extensions: [extension] }],
  });
}

export async function copyText(text: string): Promise<void> {
  if (browserPreview) {
    if (typeof navigator === "undefined" || !navigator.clipboard?.writeText) {
      throw previewError("clipboardUnavailable", "Clipboard access is unavailable.");
    }
    await navigator.clipboard.writeText(text);
    return;
  }
  await writeText(text);
}

export async function analyzePathEntries(entries: string[]): Promise<PathEntryStatus[]> {
  if (browserPreview) {
    const duplicates = new Set(duplicatePathEntryIndexes(entries));
    return entries.map((value, index) => {
      const expandedValue = value
        .replace(/%SystemRoot%/gi, "C:\\Windows")
        .replace(/%JAVA_HOME%/gi, "C:\\Program Files\\Java\\jdk-21")
        .replace(/%USERPROFILE%/gi, "C:\\Users\\developer");
      const normalized = normalizePathEntry(expandedValue);
      return {
        value,
        expandedValue,
        exists:
          normalized.startsWith("c:\\windows") || normalized.includes("program files\\java"),
        duplicate: duplicates.has(index),
      };
    });
  }
  return invoke<PathEntryStatus[]>("analyze_path_entries", { entries });
}

export async function restartElevated(): Promise<void> {
  if (browserPreview) {
    previewSnapshot.isElevated = true;
    return;
  }
  return invoke<void>("restart_elevated");
}

export function apiErrorMessage(error: unknown): string {
  if (isApiError(error)) return error.message;
  return error instanceof Error ? error.message : String(error);
}

export function desktopErrorMessage(payload: ApiError): string {
  return apiErrorMessage(payload);
}

export function apiErrorCode(error: unknown): string | null {
  return isApiError(error) ? error.code : null;
}

function scopeVariables(scope: EnvironmentScope): EnvironmentVariable[] {
  return scope === "user" ? previewSnapshot.userVariables : previewSnapshot.systemVariables;
}

function setScopeVariables(scope: EnvironmentScope, variables: EnvironmentVariable[]) {
  if (scope === "user") previewSnapshot.userVariables = variables;
  else previewSnapshot.systemVariables = variables;
}

function createPreviewBackup(scope: EnvironmentScope, reason: string): string {
  const id = `${Date.now()}-${scope}-${previewSnapshot.backups.length}.json`;
  previewBackupValues.set(id, structuredClone(scopeVariables(scope)));
  previewSnapshot.backups.unshift({
    id,
    createdAtMs: Date.now(),
    scope,
    reason,
    variableCount: scopeVariables(scope).length,
  });
  return id;
}

function previewMutationResult(undoBackupIds: string[]): MutationResult {
  previewRevision += 1;
  refreshPreviewDerivedState();
  return { snapshot: structuredClone(previewSnapshot), undoBackupIds };
}

function refreshPreviewDerivedState() {
  previewSnapshot.revision = previewRevisionValue();
  previewSnapshot.effectiveVariables = mergeEffectiveVariables(
    previewSnapshot.userVariables,
    previewSnapshot.systemVariables,
  ).map(({ name, value, valueType, source, shadowed, conflict }) => ({
    name,
    value,
    valueType,
    source,
    shadowed,
    conflict,
  }));
}

function previewRevisionValue(): string {
  return `preview-${previewRevision}`;
}

function previewImportToken(request: ImportFileRequest): string {
  return `${request.path}:${request.format}:${request.defaultScope ?? "none"}:${previewSnapshot.revision}`;
}

function reconcilePreviewFavorites(favorites: FavoriteKey[]): FavoriteKey[] {
  return favorites
    .flatMap((favorite) => {
      const variable = scopeVariables(favorite.scope).find((item) =>
        equalNames(item.name, favorite.name),
      );
      return variable ? [{ scope: favorite.scope, name: variable.name }] : [];
    })
    .sort(compareFavorites);
}

function compareFavorites(left: FavoriteKey, right: FavoriteKey): number {
  return left.scope.localeCompare(right.scope) ||
    left.name.localeCompare(right.name, undefined, { sensitivity: "base" });
}

function assertPreviewPermission(scope: EnvironmentScope) {
  if (scope === "system" && !previewSnapshot.isElevated) {
    throw previewError("elevationRequired", "Administrator permission is required.");
  }
}

function assertPreviewRevision(expectedRevision: string) {
  if (previewSnapshot.revision !== expectedRevision) {
    throw previewError(
      "environmentChanged",
      "Environment variables changed. Refresh and try again.",
    );
  }
}

function validatePreviewName(name: string) {
  if (!name || name.includes("=") || name.includes("\0")) {
    throw previewError("invalidVariable", "Variable name is invalid.");
  }
}

function equalNames(left: string, right: string): boolean {
  return previewVariableNamesEqual(left, right);
}

function previewError(code: string, message: string): ApiError {
  return { code, message };
}

function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value
  );
}
