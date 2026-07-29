export type EnvironmentScope = "user" | "system";
export type EnvironmentValueType = "string" | "expandableString";

export interface EnvironmentVariable {
  name: string;
  value: string;
  valueType: EnvironmentValueType;
  scope: EnvironmentScope;
}

export interface EnvironmentVariableInput {
  originalName: string | null;
  name: string;
  value: string;
  valueType: EnvironmentValueType;
  scope: EnvironmentScope;
}

export interface BackupSummary {
  id: string;
  createdAtMs: number;
  scope: EnvironmentScope;
  reason: string;
  variableCount: number;
}

export interface EnvironmentSnapshot {
  userVariables: EnvironmentVariable[];
  systemVariables: EnvironmentVariable[];
  effectiveVariables: EffectiveEnvironmentVariable[];
  revision: string;
  isElevated: boolean;
  backups: BackupSummary[];
  backupDirectory: string;
}

export interface PathEntryStatus {
  value: string;
  expandedValue: string;
  exists: boolean;
  duplicate: boolean;
}

export type PathStatusFilter = "all" | "duplicate" | "missing";

export interface PathEntryDraft {
  id: string;
  value: string;
}

export interface PathMovedEntry {
  value: string;
  fromIndex: number;
  toIndex: number;
}

export interface PathChangeSummary {
  added: string[];
  removed: string[];
  moved: PathMovedEntry[];
  orderChanged: boolean;
}

export type EffectiveVariableSource = EnvironmentScope | "combined";

export interface EffectiveEnvironmentVariable {
  name: string;
  value: string;
  valueType: EnvironmentValueType;
  source: EffectiveVariableSource;
  shadowed: boolean;
  conflict: boolean;
}

export interface MutationResult {
  snapshot: EnvironmentSnapshot;
  undoBackupIds: string[];
}

export type TransferMode = "copy" | "move";

export interface TransferVariableInput {
  sourceScope: EnvironmentScope;
  targetScope: EnvironmentScope;
  name: string;
  mode: TransferMode;
  overwrite: boolean;
}

export interface FavoriteKey {
  scope: EnvironmentScope;
  name: string;
}

export type TransferFileFormat = "json" | "dotEnv" | "registry";
export type ImportConflictStrategy = "skipExisting" | "overwrite";
export type ImportAction = "create" | "update" | "unchanged";

export interface ImportFileRequest {
  path: string;
  format: TransferFileFormat;
  defaultScope: EnvironmentScope | null;
}

export interface ExportFileRequest {
  path: string;
  format: TransferFileFormat;
  scope: EnvironmentScope | null;
}

export interface ImportPreviewItem {
  variable: EnvironmentVariable;
  existing: EnvironmentVariable | null;
  action: ImportAction;
}

export interface ImportPreview {
  token: string;
  environmentRevision: string;
  items: ImportPreviewItem[];
}

export interface ExportSummary {
  path: string;
  variableCount: number;
}

export type VariableCopyFormat = "name" | "value" | "powershell";

export interface ApiError {
  code: string;
  message: string;
}

export type CommandShimStatus =
  | "ready"
  | "missingExecutable"
  | "missingTarget"
  | "nameConflict"
  | "externallyModified"
  | "missingShim";

export interface CommandShimInput {
  id: string | null;
  commandName: string;
  executable: string;
  fixedArguments: string[];
}

export interface CommandShim {
  id: string;
  commandName: string;
  executable: string;
  fixedArguments: string[];
  shimPath: string;
  status: CommandShimStatus;
  statusMessage: string | null;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface CommandShimSnapshot {
  items: CommandShim[];
  managedDirectory: string;
  pathReady: boolean;
}
