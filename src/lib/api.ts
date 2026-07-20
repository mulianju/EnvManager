import { invoke } from "@tauri-apps/api/core";
import type {
  ApiError,
  EnvironmentScope,
  EnvironmentSnapshot,
  EnvironmentVariable,
  EnvironmentVariableInput,
  PathEntryStatus,
} from "../types";
import { duplicatePathEntryIndexes, normalizePathEntry } from "./environment";

const browserPreview = typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

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
  isElevated: false,
  backups: [],
  backupDirectory: "%APPDATA%\\EnvManager\\backups",
};
const previewBackupValues = new Map<string, EnvironmentVariable[]>();

export async function getEnvironmentSnapshot(): Promise<EnvironmentSnapshot> {
  if (browserPreview) return structuredClone(previewSnapshot);
  return invoke<EnvironmentSnapshot>("get_environment_snapshot");
}

export async function saveEnvironmentVariable(
  input: EnvironmentVariableInput,
): Promise<EnvironmentSnapshot> {
  if (browserPreview) {
    assertPreviewPermission(input.scope);
    createPreviewBackup(input.scope, "beforeSet");
    const variables = scopeVariables(input.scope);
    if (!input.originalName && variables.some((item) => equalNames(item.name, input.name))) {
      throw previewError("variableAlreadyExists", `${input.name} already exists.`);
    }
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
    return structuredClone(previewSnapshot);
  }
  return invoke<EnvironmentSnapshot>("save_environment_variable", { input });
}

export async function deleteEnvironmentVariable(
  scope: EnvironmentScope,
  name: string,
): Promise<EnvironmentSnapshot> {
  if (browserPreview) {
    assertPreviewPermission(scope);
    createPreviewBackup(scope, "beforeDelete");
    setScopeVariables(
      scope,
      scopeVariables(scope).filter((item) => !equalNames(item.name, name)),
    );
    return structuredClone(previewSnapshot);
  }
  return invoke<EnvironmentSnapshot>("delete_environment_variable", { scope, name });
}

export async function restoreEnvironmentBackup(backupId: string): Promise<EnvironmentSnapshot> {
  if (browserPreview) {
    const backup = previewSnapshot.backups.find((item) => item.id === backupId);
    if (!backup) throw previewError("backupOperationFailed", "Backup was not found.");
    assertPreviewPermission(backup.scope);
    const variables = previewBackupValues.get(backupId);
    if (!variables) throw previewError("backupOperationFailed", "Backup contents were not found.");
    createPreviewBackup(backup.scope, "beforeRestore");
    setScopeVariables(backup.scope, structuredClone(variables));
    return structuredClone(previewSnapshot);
  }
  return invoke<EnvironmentSnapshot>("restore_environment_backup", { backupId });
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
          normalized.startsWith("c:\\windows") ||
          normalized.includes("program files\\java"),
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

function scopeVariables(scope: EnvironmentScope): EnvironmentVariable[] {
  return scope === "user" ? previewSnapshot.userVariables : previewSnapshot.systemVariables;
}

function setScopeVariables(scope: EnvironmentScope, variables: EnvironmentVariable[]) {
  if (scope === "user") previewSnapshot.userVariables = variables;
  else previewSnapshot.systemVariables = variables;
}

function createPreviewBackup(scope: EnvironmentScope, reason: string) {
  const id = `${Date.now()}-${scope}-${previewSnapshot.backups.length}.json`;
  previewBackupValues.set(id, structuredClone(scopeVariables(scope)));
  previewSnapshot.backups.unshift({
    id,
    createdAtMs: Date.now(),
    scope,
    reason,
    variableCount: scopeVariables(scope).length,
  });
}

function assertPreviewPermission(scope: EnvironmentScope) {
  if (scope === "system" && !previewSnapshot.isElevated) {
    throw previewError("elevationRequired", "Administrator permission is required.");
  }
}

function equalNames(left: string, right: string): boolean {
  return left.toLowerCase() === right.toLowerCase();
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
