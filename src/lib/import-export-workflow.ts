import type {
  EnvironmentScope,
  ExportFileRequest,
  ImportConflictStrategy,
  ImportFileRequest,
  ImportPreview,
  TransferFileFormat,
} from "../types";

export interface ImportPreviewSummary {
  create: number;
  update: number;
  unchanged: number;
  total: number;
  items: ImportPreview["items"];
}

export function deriveTransferFileFormat(
  path: string,
  selectedFormat: TransferFileFormat | null,
): TransferFileFormat | null {
  if (selectedFormat) return selectedFormat;
  const extension = path.trim().match(/\.([^.\\/]+)$/)?.[1].toLowerCase();
  if (extension === "json") return "json";
  if (extension === "env") return "dotEnv";
  if (extension === "reg") return "registry";
  return null;
}

export function createImportRequest(
  path: string,
  selectedFormat: TransferFileFormat | null,
  defaultScope: EnvironmentScope | null,
): ImportFileRequest | null {
  const normalizedPath = path.trim();
  const format = deriveTransferFileFormat(normalizedPath, selectedFormat);
  if (!normalizedPath || !format) return null;
  if (format === "dotEnv" && !defaultScope) return null;
  return {
    path: normalizedPath,
    format,
    defaultScope: format === "dotEnv" ? defaultScope : null,
  };
}

export function summarizeImportPreview(
  preview: ImportPreview,
): ImportPreviewSummary {
  const summary: ImportPreviewSummary = {
    create: 0,
    update: 0,
    unchanged: 0,
    total: preview.items.length,
    items: preview.items,
  };
  preview.items.forEach(({ action }) => {
    summary[action] += 1;
  });
  return summary;
}

export function importConfirmationMessage(
  preview: ImportPreview,
  strategy: ImportConflictStrategy,
): string {
  const summary = summarizeImportPreview(preview);
  const updateImpact = strategy === "overwrite"
    ? `overwrite ${summary.update}`
    : `skip ${summary.update}`;
  return `Apply this import: create ${summary.create}, ${updateImpact}, ${summary.unchanged} unchanged?`;
}

export function previewWritesSystem(
  preview: ImportPreview | null,
  strategy: ImportConflictStrategy | null,
): boolean {
  if (!preview) return false;
  return preview.items.some(({ variable, action }) =>
    variable.scope === "system" &&
    (action === "create" || (action === "update" && strategy === "overwrite")),
  );
}

export function createExportRequest(
  path: string,
  format: TransferFileFormat,
  scope: EnvironmentScope | null,
): ExportFileRequest | null {
  const normalizedPath = path.trim();
  if (!normalizedPath || (format === "dotEnv" && !scope)) return null;
  return { path: normalizedPath, format, scope };
}

export function defaultExportFileName(
  format: TransferFileFormat,
  scope: EnvironmentScope | null,
): string {
  const extension = format === "dotEnv" ? "env" : format === "registry" ? "reg" : "json";
  return `environment-${scope ?? "all"}.${extension}`;
}
