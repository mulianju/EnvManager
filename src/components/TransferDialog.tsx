import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import {
  AlertCircle,
  Check,
  FileDown,
  FileSearch,
  FileUp,
  LoaderCircle,
  RefreshCw,
  ShieldCheck,
  X,
} from "lucide-react";
import {
  apiErrorCode,
  apiErrorMessage,
  applyEnvironmentImport,
  exportEnvironmentFile,
  pickExportFile,
  pickImportFile,
  previewEnvironmentImport,
} from "../lib/api";
import {
  createExportRequest,
  createImportRequest,
  defaultExportFileName,
  deriveTransferFileFormat,
  importConfirmationMessage,
  previewWritesSystem,
  summarizeImportPreview,
} from "../lib/import-export-workflow";
import type {
  EnvironmentScope,
  ImportConflictStrategy,
  ImportFileRequest,
  ImportPreview,
  MutationResult,
  TransferFileFormat,
} from "../types";

export type TransferDialogMode = "import" | "export";

interface TransferDialogProps {
  mode: TransferDialogMode;
  isElevated: boolean;
  onClose: () => void;
  onImported: (result: MutationResult, message: string) => void;
  onNotice: (message: string) => void;
  onRestartElevated: () => Promise<void>;
}

export function TransferDialog({
  mode,
  isElevated,
  onClose,
  onImported,
  onNotice,
  onRestartElevated,
}: TransferDialogProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    dialogRef.current
      ?.querySelector<HTMLElement>("[data-autofocus]")
      ?.focus();
  }, [mode]);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [busy, onClose]);

  return (
    <div className="modal-backdrop" role="presentation">
      <section
        aria-labelledby="transfer-title"
        aria-modal="true"
        className="editor-modal transfer-modal"
        onKeyDown={(event) => trapDialogFocus(event, dialogRef.current)}
        ref={dialogRef}
        role="dialog"
      >
        <header className="modal-header">
          <div>
            <h2 id="transfer-title">
              {mode === "import" ? "Import environment" : "Export environment"}
            </h2>
            <span>
              {mode === "import"
                ? "Preview every change before applying"
                : "Save a portable environment file"}
            </span>
          </div>
          <button
            aria-label={`Close ${mode} dialog`}
            className="icon-button"
            disabled={busy}
            onClick={onClose}
            type="button"
          >
            <X size={17} />
          </button>
        </header>

        {mode === "import" ? (
          <ImportWorkflow
            busy={busy}
            error={error}
            isElevated={isElevated}
            onBusyChange={setBusy}
            onClose={onClose}
            onError={setError}
            onImported={onImported}
            onRestartElevated={onRestartElevated}
          />
        ) : (
          <ExportWorkflow
            busy={busy}
            error={error}
            onBusyChange={setBusy}
            onClose={onClose}
            onError={setError}
            onNotice={onNotice}
          />
        )}
      </section>
    </div>
  );
}

interface WorkflowStateProps {
  busy: boolean;
  error: string | null;
  onBusyChange: (busy: boolean) => void;
  onClose: () => void;
  onError: (error: string | null) => void;
}

function ImportWorkflow({
  busy,
  error,
  isElevated,
  onBusyChange,
  onClose,
  onError,
  onImported,
  onRestartElevated,
}: WorkflowStateProps & {
  isElevated: boolean;
  onImported: TransferDialogProps["onImported"];
  onRestartElevated: TransferDialogProps["onRestartElevated"];
}) {
  const [path, setPath] = useState<string | null>(null);
  const [format, setFormat] = useState<TransferFileFormat | null>(null);
  const [defaultScope, setDefaultScope] = useState<EnvironmentScope | null>(null);
  const [request, setRequest] = useState<ImportFileRequest | null>(null);
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [strategy, setStrategy] = useState<ImportConflictStrategy | null>(null);
  const summary = useMemo(
    () => preview ? summarizeImportPreview(preview) : null,
    [preview],
  );
  const requiresElevation = !isElevated && previewWritesSystem(preview, strategy);
  const requestReady = Boolean(
    path && format && (format !== "dotEnv" || defaultScope),
  );
  const applyReady = Boolean(
    preview &&
    preview.items.length > 0 &&
    (!summary?.update || strategy) &&
    !requiresElevation,
  );

  const chooseFile = async () => {
    onError(null);
    onBusyChange(true);
    try {
      const selected = await pickImportFile();
      if (!selected) return;
      const selectedFormat = deriveTransferFileFormat(selected, null);
      if (!selectedFormat) {
        setPath(null);
        setFormat(null);
        setRequest(null);
        setPreview(null);
        setStrategy(null);
        onError("Unsupported file type. Choose a .json, .env, or .reg file.");
        return;
      }
      setPath(selected);
      setFormat(selectedFormat);
      setDefaultScope(selectedFormat === "dotEnv" ? null : defaultScope);
      setRequest(null);
      setPreview(null);
      setStrategy(null);
    } catch (nextError) {
      onError(`File selection failed: ${apiErrorMessage(nextError)}`);
    } finally {
      onBusyChange(false);
    }
  };

  const loadPreview = async () => {
    if (!path || !format || busy) return;
    const nextRequest = createImportRequest(path, format, defaultScope);
    if (!nextRequest) {
      onError(
        format === "dotEnv"
          ? "Choose whether the .env file should import into User or System variables."
          : "Choose a supported import file.",
      );
      return;
    }
    onError(null);
    setPreview(null);
    setStrategy(null);
    onBusyChange(true);
    try {
      const nextPreview = await previewEnvironmentImport(nextRequest);
      setRequest(nextRequest);
      setPreview(nextPreview);
    } catch (nextError) {
      setRequest(null);
      onError(`Import preview failed: ${apiErrorMessage(nextError)}`);
    } finally {
      onBusyChange(false);
    }
  };

  const applyImport = async () => {
    if (!request || !preview || !applyReady || busy) return;
    const selectedStrategy = strategy ?? "skipExisting";
    if (
      summary &&
      summary.update > 0 &&
      !window.confirm(importConfirmationMessage(preview, selectedStrategy))
    ) {
      return;
    }
    onError(null);
    onBusyChange(true);
    try {
      const result = await applyEnvironmentImport(
        request,
        selectedStrategy,
        preview.token,
        preview.environmentRevision,
      );
      const appliedCount = (summary?.create ?? 0) +
        (selectedStrategy === "overwrite" ? summary?.update ?? 0 : 0);
      onImported(
        result,
        `Imported ${appliedCount} ${appliedCount === 1 ? "variable" : "variables"}.`,
      );
      onClose();
    } catch (nextError) {
      if (apiErrorCode(nextError) === "importPreviewChanged") {
        setRequest(null);
        setPreview(null);
        setStrategy(null);
        onError("The file or environment changed after preview. Preview the import again.");
      } else {
        onError(`Import failed: ${apiErrorMessage(nextError)}`);
      }
    } finally {
      onBusyChange(false);
    }
  };

  const restartElevatedForImport = async () => {
    if (busy) return;
    onError(null);
    onBusyChange(true);
    try {
      await onRestartElevated();
    } catch (nextError) {
      onError(`Elevation failed: ${apiErrorMessage(nextError)}`);
    } finally {
      onBusyChange(false);
    }
  };

  const selectDefaultScope = (scope: EnvironmentScope | null) => {
    setDefaultScope(scope);
    setRequest(null);
    setPreview(null);
    setStrategy(null);
    onError(null);
  };

  return (
    <>
      <div className="modal-body transfer-body">
        <div className="transfer-file-row">
          <button
            className="secondary-button"
            data-autofocus
            disabled={busy}
            onClick={() => void chooseFile()}
            type="button"
          >
            {busy && !path
              ? <LoaderCircle className="spin" size={16} />
              : <FileSearch size={16} />}
            {path ? "Choose another file" : "Choose import file"}
          </button>
          <span className="transfer-path" title={path ?? undefined}>
            {path ?? "JSON, .env, or Registry file"}
          </span>
        </div>

        {format && (
          <div className="transfer-options">
            <div className="transfer-format">
              <small>Detected format</small>
              <strong>{formatLabel(format)}</strong>
            </div>
            {format === "dotEnv" && (
              <label>
                <span>Import into</span>
                <select
                  disabled={busy}
                  onChange={(event) => selectDefaultScope(
                    event.target.value ? event.target.value as EnvironmentScope : null,
                  )}
                  value={defaultScope ?? ""}
                >
                  <option value="">Choose scope</option>
                  <option value="user">User variables</option>
                  <option value="system">System variables</option>
                </select>
              </label>
            )}
            <button
              className="primary-button"
              disabled={busy || !requestReady}
              onClick={() => void loadPreview()}
              type="button"
            >
              {busy
                ? <LoaderCircle className="spin" size={16} />
                : <RefreshCw size={16} />}
              Preview import
            </button>
          </div>
        )}

        {error && <TransferError>{error}</TransferError>}
        {busy && path && !preview && (
          <div className="transfer-loading" role="status">
            <LoaderCircle className="spin" size={18} /> Reading and comparing variables...
          </div>
        )}
        {preview && summary && (
          <div className="import-preview">
            <div className="import-summary" role="status">
              <SummaryCount count={summary.create} label="Create" />
              <SummaryCount count={summary.update} label="Update" />
              <SummaryCount count={summary.unchanged} label="Unchanged" />
            </div>
            {summary.total === 0 ? (
              <div className="transfer-empty">
                <Check size={20} />
                <strong>No variables found</strong>
                <span>The selected file contains no supported environment values.</span>
              </div>
            ) : (
              <div className="import-preview-list" aria-label="Import preview">
                <div className="import-preview-header">
                  <span>Variable</span><span>Change</span><span>Current value</span><span>New value</span><span>Type</span>
                </div>
                {preview.items.map((item, index) => (
                  <div
                    className={`import-preview-row ${item.action}`}
                    key={`${item.variable.scope}:${item.variable.name}:${index}`}
                  >
                    <div><small>Variable</small><strong>{item.variable.name}</strong><span className={`scope-label ${item.variable.scope}`}>{item.variable.scope}</span></div>
                    <div><small>Change</small><span className={`action-label ${item.action}`}>{actionLabel(item.action)}</span></div>
                    <div><small>Current value</small><code>{item.existing?.value ?? "Not present"}</code></div>
                    <div><small>New value</small><code>{item.variable.value || "(empty)"}</code></div>
                    <div><small>Type</small><span>{valueTypeLabel(item.variable.valueType)}</span></div>
                  </div>
                ))}
              </div>
            )}

            {summary.update > 0 && (
              <fieldset className="conflict-strategy">
                <legend>Existing variables</legend>
                <label>
                  <input
                    checked={strategy === "skipExisting"}
                    disabled={busy}
                    name="import-strategy"
                    onChange={() => setStrategy("skipExisting")}
                    type="radio"
                  />
                  <span><strong>Skip existing</strong><small>Only create {summary.create} new variables; keep current values.</small></span>
                </label>
                <label>
                  <input
                    checked={strategy === "overwrite"}
                    disabled={busy}
                    name="import-strategy"
                    onChange={() => setStrategy("overwrite")}
                    type="radio"
                  />
                  <span><strong>Overwrite existing</strong><small>Create {summary.create} and replace {summary.update} current values.</small></span>
                </label>
              </fieldset>
            )}

            {requiresElevation && (
              <div className="transfer-permission" role="alert">
                <ShieldCheck size={19} />
                <div><strong>Administrator permission required</strong><span>The selected strategy writes System variables. Restart as administrator before applying it.</span></div>
                <button className="secondary-button" disabled={busy} onClick={() => void restartElevatedForImport()} type="button"><ShieldCheck size={15} /> Restart</button>
              </div>
            )}
          </div>
        )}
      </div>
      <footer className="modal-footer">
        <span className="transfer-footer-note">
          {preview && summary && summary.update > 0 && !strategy
            ? "Choose how to handle existing variables."
            : "A backup is created before values change."}
        </span>
        <div className="button-row">
          <button className="secondary-button" disabled={busy} onClick={onClose} type="button">Cancel</button>
          <button className="primary-button" disabled={busy || !applyReady} onClick={() => void applyImport()} type="button">
            {busy ? <LoaderCircle className="spin" size={16} /> : <FileUp size={16} />}
            Apply import
          </button>
        </div>
      </footer>
    </>
  );
}

function ExportWorkflow({
  busy,
  error,
  onBusyChange,
  onClose,
  onError,
  onNotice,
}: WorkflowStateProps & { onNotice: TransferDialogProps["onNotice"] }) {
  const [format, setFormat] = useState<TransferFileFormat>("json");
  const [scope, setScope] = useState<EnvironmentScope | null>(null);
  const canExport = format !== "dotEnv" || scope !== null;

  const selectFormat = (nextFormat: TransferFileFormat) => {
    setFormat(nextFormat);
    if (nextFormat === "dotEnv" && scope === null) setScope(null);
    onError(null);
  };

  const exportFile = async () => {
    if (!canExport || busy) return;
    onError(null);
    onBusyChange(true);
    try {
      const path = await pickExportFile(
        format,
        defaultExportFileName(format, scope),
      );
      if (!path) return;
      const request = createExportRequest(path, format, scope);
      if (!request) {
        onError("Choose User or System scope before exporting a .env file.");
        return;
      }
      const result = await exportEnvironmentFile(request);
      onNotice(
        `Exported ${result.variableCount} ${result.variableCount === 1 ? "variable" : "variables"} to ${result.path}.`,
      );
      onClose();
    } catch (nextError) {
      onError(`Export failed: ${apiErrorMessage(nextError)}`);
    } finally {
      onBusyChange(false);
    }
  };

  return (
    <>
      <div className="modal-body transfer-body">
        <div className="export-options">
          <label>
            <span>File format</span>
            <select
              data-autofocus
              disabled={busy}
              onChange={(event) => selectFormat(event.target.value as TransferFileFormat)}
              value={format}
            >
              <option value="json">JSON</option>
              <option value="dotEnv">.env</option>
              <option value="registry">Registry (.reg)</option>
            </select>
          </label>
          <label>
            <span>Variables to export</span>
            <select
              disabled={busy}
              onChange={(event) => setScope(
                event.target.value ? event.target.value as EnvironmentScope : null,
              )}
              value={scope ?? ""}
            >
              {format !== "dotEnv" && <option value="">User and System</option>}
              {format === "dotEnv" && <option value="">Choose scope</option>}
              <option value="user">User variables</option>
              <option value="system">System variables</option>
            </select>
          </label>
        </div>
        <div className="export-format-note">
          <FileDown size={19} />
          <div>
            <strong>{formatLabel(format)}</strong>
            <span>{formatDescription(format)}</span>
            <code>{defaultExportFileName(format, scope)}</code>
          </div>
        </div>
        {error && <TransferError>{error}</TransferError>}
      </div>
      <footer className="modal-footer">
        <span className="transfer-footer-note">
          Export reads System values without requiring administrator permission.
        </span>
        <div className="button-row">
          <button className="secondary-button" disabled={busy} onClick={onClose} type="button">Cancel</button>
          <button className="primary-button" disabled={busy || !canExport} onClick={() => void exportFile()} type="button">
            {busy ? <LoaderCircle className="spin" size={16} /> : <FileDown size={16} />}
            Choose location
          </button>
        </div>
      </footer>
    </>
  );
}

function TransferError({ children }: { children: string }) {
  return <div className="transfer-error" role="alert"><AlertCircle size={16} /><span>{children}</span></div>;
}

function SummaryCount({ count, label }: { count: number; label: string }) {
  return <div><strong>{count}</strong><span>{label}</span></div>;
}

function formatLabel(format: TransferFileFormat): string {
  return { json: "JSON", dotEnv: ".env", registry: "Registry (.reg)" }[format];
}

function formatDescription(format: TransferFileFormat): string {
  return {
    json: "Preserves scope and Registry value type for a complete round trip.",
    dotEnv: "Exports one selected scope as portable NAME=VALUE entries.",
    registry: "Creates a Windows Registry file for the selected environment scopes.",
  }[format];
}

function actionLabel(action: ImportPreview["items"][number]["action"]): string {
  return { create: "Create", update: "Update", unchanged: "Unchanged" }[action];
}

function valueTypeLabel(type: ImportPreview["items"][number]["variable"]["valueType"]): string {
  return type === "expandableString" ? "Expandable" : "String";
}

function trapDialogFocus(
  event: ReactKeyboardEvent<HTMLElement>,
  dialog: HTMLElement | null,
) {
  if (event.key !== "Tab" || !dialog) return;
  const focusable = Array.from(
    dialog.querySelectorAll<HTMLElement>(
      "button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex='-1'])",
    ),
  );
  if (focusable.length === 0) return;
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (event.shiftKey && (document.activeElement === first || !dialog.contains(document.activeElement))) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}
