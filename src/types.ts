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

export interface ApiError {
  code: string;
  message: string;
}
