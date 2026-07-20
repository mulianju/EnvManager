import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  ArrowDown,
  ArrowUp,
  Check,
  DatabaseBackup,
  Eye,
  EyeOff,
  FolderCheck,
  FolderX,
  History,
  LoaderCircle,
  LockKeyhole,
  Pencil,
  Plus,
  RefreshCw,
  Save,
  Search,
  Shield,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  User,
  Variable,
  X,
} from "lucide-react";
import "./App.css";
import {
  analyzePathEntries,
  apiErrorMessage,
  deleteEnvironmentVariable,
  getEnvironmentSnapshot,
  restartElevated,
  restoreEnvironmentBackup,
  saveEnvironmentVariable,
} from "./lib/api";
import {
  filterVariables,
  isPathVariable,
  isSensitiveVariable,
  joinPathEntries,
  parsePathEntries,
} from "./lib/environment";
import type {
  BackupSummary,
  EnvironmentScope,
  EnvironmentSnapshot,
  EnvironmentVariable,
  EnvironmentVariableInput,
  PathEntryStatus,
} from "./types";

type View = EnvironmentScope | "backups";

const navigation: Array<{ id: View; label: string; icon: typeof User }> = [
  { id: "user", label: "User variables", icon: User },
  { id: "system", label: "System variables", icon: Shield },
  { id: "backups", label: "Backups", icon: History },
];

function App() {
  const [view, setView] = useState<View>("user");
  const [snapshot, setSnapshot] = useState<EnvironmentSnapshot | null>(null);
  const [query, setQuery] = useState("");
  const [editor, setEditor] = useState<EnvironmentVariableInput | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const requestId = useRef(0);

  const refresh = useCallback(async () => {
    const currentRequest = ++requestId.current;
    setLoading(true);
    setError(null);
    try {
      const next = await getEnvironmentSnapshot();
      if (requestId.current === currentRequest) setSnapshot(next);
    } catch (nextError) {
      if (requestId.current === currentRequest) setError(apiErrorMessage(nextError));
    } finally {
      if (requestId.current === currentRequest) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const perform = async <T,>(action: () => Promise<T>): Promise<T | null> => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      return await action();
    } catch (nextError) {
      setError(apiErrorMessage(nextError));
      return null;
    } finally {
      setBusy(false);
    }
  };

  const scope = view === "backups" ? null : view;
  const variables = useMemo(() => {
    if (!snapshot || !scope) return [];
    return filterVariables(
      scope === "user" ? snapshot.userVariables : snapshot.systemVariables,
      query,
    );
  }, [snapshot, scope, query]);

  const openVariable = (variable: EnvironmentVariable) => {
    setEditor({
      originalName: variable.name,
      name: variable.name,
      value: variable.value,
      valueType: variable.valueType,
      scope: variable.scope,
    });
  };

  const addVariable = () => {
    if (!scope) return;
    setEditor({
      originalName: null,
      name: "",
      value: "",
      valueType: "string",
      scope,
    });
  };

  const saveVariable = async (input: EnvironmentVariableInput) => {
    const next = await perform(() => saveEnvironmentVariable(input));
    if (next) {
      setSnapshot(next);
      setEditor(null);
      setNotice(input.originalName ? "Variable updated." : "Variable created.");
    }
  };

  const deleteVariable = async (input: EnvironmentVariableInput) => {
    if (!input.originalName) {
      setEditor(null);
      return;
    }
    if (!window.confirm(`Delete ${input.originalName} from ${input.scope} variables?`)) return;
    const next = await perform(() =>
      deleteEnvironmentVariable(input.scope, input.originalName ?? input.name),
    );
    if (next) {
      setSnapshot(next);
      setEditor(null);
      setNotice("Variable deleted. A backup was created.");
    }
  };

  const elevate = async () => {
    const result = await perform(restartElevated);
    if (result !== null) await refresh();
  };

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark"><Variable size={18} /></span>
          <span>EnvManager</span>
        </div>
        <nav className="nav-list" aria-label="Primary navigation">
          {navigation.map((item) => {
            const Icon = item.icon;
            const count = snapshot
              ? item.id === "user"
                ? snapshot.userVariables.length
                : item.id === "system"
                  ? snapshot.systemVariables.length
                  : snapshot.backups.length
              : 0;
            return (
              <button
                aria-label={item.label}
                className={view === item.id ? "nav-item active" : "nav-item"}
                key={item.id}
                onClick={() => {
                  setView(item.id);
                  setQuery("");
                }}
                type="button"
              >
                <Icon size={17} />
                <span>{item.label}</span>
                <small>{count}</small>
              </button>
            );
          })}
        </nav>
        <div className={snapshot?.isElevated ? "access-status elevated" : "access-status"}>
          {snapshot?.isElevated ? <ShieldCheck size={16} /> : <LockKeyhole size={16} />}
          <span>{snapshot?.isElevated ? "Administrator" : "Standard access"}</span>
        </div>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div>
            <h1>{viewTitle(view)}</h1>
            <p>{viewSubtitle(view)}</p>
          </div>
          <div className="topbar-actions">
            {scope && (
              <label className="search-control">
                <Search size={16} />
                <input aria-label="Search variables" placeholder="Search" value={query} onChange={(event) => setQuery(event.target.value)} />
              </label>
            )}
            <button className="icon-button" disabled={loading} onClick={() => void refresh()} title="Refresh" type="button">
              <RefreshCw className={loading ? "spin" : ""} size={17} />
            </button>
            {scope && (
              <button className="primary-button" disabled={scope === "system" && !snapshot?.isElevated} onClick={addVariable} type="button">
                <Plus size={16} /> Add variable
              </button>
            )}
          </div>
        </header>

        {error && <Message kind="error" onClose={() => setError(null)}>{error}</Message>}
        {notice && <Message kind="success" onClose={() => setNotice(null)}>{notice}</Message>}

        {loading && !snapshot ? (
          <div className="loading-state"><LoaderCircle className="spin" size={24} /> Loading environment</div>
        ) : snapshot ? (
          <>
            {view === "system" && !snapshot.isElevated && (
              <div className="permission-banner">
                <ShieldAlert size={20} />
                <div><strong>System variables are read-only</strong><span>Restart with administrator permission to create, edit, delete, or restore system values.</span></div>
                <button className="secondary-button" disabled={busy} onClick={() => void elevate()} type="button"><ShieldCheck size={16} /> Restart as administrator</button>
              </div>
            )}
            {scope ? (
              <VariablesView
                canEdit={scope === "user" || snapshot.isElevated}
                onOpen={openVariable}
                query={query}
                variables={variables}
              />
            ) : (
              <BackupsView
                busy={busy}
                onRestore={async (backup) => {
                  if (backup.scope === "system" && !snapshot.isElevated) {
                    setError("Restart as administrator before restoring a system backup.");
                    return;
                  }
                  if (!window.confirm(`Restore this ${backup.scope} backup? Current values in that scope will be replaced.`)) return;
                  const next = await perform(() => restoreEnvironmentBackup(backup.id));
                  if (next) {
                    setSnapshot(next);
                    setNotice("Backup restored. A rollback backup was created.");
                  }
                }}
                snapshot={snapshot}
              />
            )}
          </>
        ) : (
          <div className="empty-state"><AlertCircle size={26} /><strong>Unable to read environment variables</strong><button className="primary-button" onClick={() => void refresh()} type="button"><RefreshCw size={16} /> Retry</button></div>
        )}
      </main>

      {editor && (
        <VariableEditor
          busy={busy}
          input={editor}
          onClose={() => setEditor(null)}
          onDelete={() => void deleteVariable(editor)}
          onSave={(input) => void saveVariable(input)}
        />
      )}
    </div>
  );
}

function VariablesView({ variables, query, canEdit, onOpen }: { variables: EnvironmentVariable[]; query: string; canEdit: boolean; onOpen: (variable: EnvironmentVariable) => void }) {
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  return (
    <section className="content-section">
      <div className="table-summary"><span>{variables.length} variables</span>{query && <span>Filtered by “{query}”</span>}</div>
      <div className="variable-table">
        <div className="variable-header"><span>Name</span><span>Value</span><span>Type</span><span /></div>
        {variables.map((variable) => {
          const sensitive = isSensitiveVariable(variable.name);
          const isRevealed = revealed.has(variable.name.toLowerCase());
          return (
            <div className="variable-row" key={variable.name}>
              <strong>{variable.name}</strong>
              <span className={sensitive && !isRevealed ? "value-preview masked" : "value-preview"}>{sensitive && !isRevealed ? "••••••••••••" : variable.value || "(empty)"}</span>
              <span className="type-label">{variable.valueType === "expandableString" ? "Expandable" : "String"}</span>
              <div className="row-actions">
                {sensitive && <button className="icon-button small" aria-label={isRevealed ? "Hide value" : "Show value"} onClick={() => setRevealed((current) => toggleSetValue(current, variable.name.toLowerCase()))} type="button">{isRevealed ? <EyeOff size={15} /> : <Eye size={15} />}</button>}
                <button className="icon-button small" aria-label={`Edit ${variable.name}`} disabled={!canEdit} onClick={() => onOpen(variable)} type="button"><Pencil size={15} /></button>
              </div>
            </div>
          );
        })}
        {variables.length === 0 && <div className="table-empty">{query ? "No matching variables" : "No variables in this scope"}</div>}
      </div>
    </section>
  );
}

function BackupsView({ snapshot, busy, onRestore }: { snapshot: EnvironmentSnapshot; busy: boolean; onRestore: (backup: BackupSummary) => void }) {
  return (
    <section className="content-section">
      <div className="backup-location"><DatabaseBackup size={17} /><span>{snapshot.backupDirectory}</span></div>
      <div className="backup-table">
        <div className="backup-header"><span>Created</span><span>Scope</span><span>Reason</span><span>Variables</span><span /></div>
        {snapshot.backups.map((backup) => (
          <div className="backup-row" key={backup.id}>
            <span>{new Date(backup.createdAtMs).toLocaleString()}</span>
            <span className={`scope-label ${backup.scope}`}>{backup.scope}</span>
            <span>{backupReason(backup.reason)}</span>
            <span>{backup.variableCount}</span>
            <button className="secondary-button compact-button" disabled={busy} onClick={() => onRestore(backup)} type="button"><History size={15} /> Restore</button>
          </div>
        ))}
        {snapshot.backups.length === 0 && <div className="table-empty">Backups appear automatically before each change</div>}
      </div>
    </section>
  );
}

function VariableEditor({ input, busy, onSave, onDelete, onClose }: { input: EnvironmentVariableInput; busy: boolean; onSave: (input: EnvironmentVariableInput) => void; onDelete: () => void; onClose: () => void }) {
  const [draft, setDraft] = useState(input);
  const [pathEntries, setPathEntries] = useState(() => parsePathEntries(input.value));
  const [statuses, setStatuses] = useState<PathEntryStatus[]>([]);
  const [checkingPaths, setCheckingPaths] = useState(false);
  const [showSensitive, setShowSensitive] = useState(false);
  const pathMode = isPathVariable(draft.name) || isPathVariable(input.originalName ?? "");

  useEffect(() => {
    if (!pathMode) return;
    setCheckingPaths(true);
    const timeout = window.setTimeout(() => {
      void analyzePathEntries(pathEntries)
        .then(setStatuses)
        .finally(() => setCheckingPaths(false));
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [pathEntries, pathMode]);

  const submit = () => {
    onSave({ ...draft, value: pathMode ? joinPathEntries(pathEntries) : draft.value });
  };

  return (
    <div className="modal-backdrop" role="presentation">
      <section aria-labelledby="editor-title" aria-modal="true" className={pathMode ? "editor-modal path-modal" : "editor-modal"} role="dialog">
        <header className="modal-header">
          <div><h2 id="editor-title">{input.originalName ? `Edit ${input.originalName}` : "New variable"}</h2><span>{draft.scope === "user" ? "User environment" : "System environment"}</span></div>
          <button aria-label="Close editor" className="icon-button" onClick={onClose} type="button"><X size={17} /></button>
        </header>
        <div className="modal-body">
          <div className="form-grid">
            <label><span>Name</span><input autoFocus value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
            <label><span>Registry type</span><select value={draft.valueType} onChange={(event) => setDraft({ ...draft, valueType: event.target.value as EnvironmentVariableInput["valueType"] })}><option value="string">String (REG_SZ)</option><option value="expandableString">Expandable string (REG_EXPAND_SZ)</option></select></label>
          </div>
          {pathMode ? (
            <PathEditor checking={checkingPaths} entries={pathEntries} onChange={setPathEntries} statuses={statuses} />
          ) : (
            <label className="value-field">
              <span>Value</span>
              <div className="value-input-wrap">
                <textarea className={isSensitiveVariable(draft.name) && !showSensitive ? "secret-masked" : ""} rows={7} value={draft.value} onChange={(event) => setDraft({ ...draft, value: event.target.value })} />
                {isSensitiveVariable(draft.name) && <button aria-label={showSensitive ? "Hide value" : "Show value"} className="icon-button small reveal-button" onClick={() => setShowSensitive((current) => !current)} type="button">{showSensitive ? <EyeOff size={15} /> : <Eye size={15} />}</button>}
              </div>
            </label>
          )}
        </div>
        <footer className="modal-footer">
          <div>{input.originalName && <button className="danger-button" disabled={busy} onClick={onDelete} type="button"><Trash2 size={16} /> Delete</button>}</div>
          <div className="button-row"><button className="secondary-button" onClick={onClose} type="button">Cancel</button><button className="primary-button" disabled={busy || !draft.name.trim()} onClick={submit} type="button">{busy ? <LoaderCircle className="spin" size={16} /> : <Save size={16} />} Save</button></div>
        </footer>
      </section>
    </div>
  );
}

function PathEditor({ entries, statuses, checking, onChange }: { entries: string[]; statuses: PathEntryStatus[]; checking: boolean; onChange: (entries: string[]) => void }) {
  const update = (index: number, value: string) => onChange(entries.map((entry, current) => current === index ? value : entry));
  const move = (index: number, direction: -1 | 1) => {
    const target = index + direction;
    if (target < 0 || target >= entries.length) return;
    const next = [...entries];
    [next[index], next[target]] = [next[target], next[index]];
    onChange(next);
  };
  return (
    <div className="path-editor">
      <div className="path-toolbar"><div><h3>PATH entries</h3><span>{entries.length} ordered entries</span></div><button className="secondary-button" onClick={() => onChange([...entries, ""])} type="button"><Plus size={15} /> Add entry</button></div>
      <div className="path-list">
        {entries.map((entry, index) => {
          const status = statuses[index];
          return (
            <div className={status?.duplicate ? "path-row duplicate" : "path-row"} key={index}>
              <span className="path-index">{index + 1}</span>
              <div className="path-input"><input aria-label={`PATH entry ${index + 1}`} value={entry} onChange={(event) => update(index, event.target.value)} />{status && <small title={status.expandedValue}>{status.duplicate ? "Duplicate entry" : status.exists ? "Path exists" : "Path not found"}</small>}</div>
              <span className={status?.duplicate || (status && !status.exists) ? "path-status warning" : "path-status valid"}>{checking && !status ? <LoaderCircle className="spin" size={15} /> : status?.exists && !status.duplicate ? <FolderCheck size={16} /> : <FolderX size={16} />}</span>
              <div className="path-actions"><button aria-label="Move up" className="icon-button small" disabled={index === 0} onClick={() => move(index, -1)} type="button"><ArrowUp size={14} /></button><button aria-label="Move down" className="icon-button small" disabled={index === entries.length - 1} onClick={() => move(index, 1)} type="button"><ArrowDown size={14} /></button><button aria-label="Remove entry" className="icon-button small danger-icon" onClick={() => onChange(entries.filter((_, current) => current !== index))} type="button"><Trash2 size={14} /></button></div>
            </div>
          );
        })}
        {entries.length === 0 && <div className="table-empty">No PATH entries</div>}
      </div>
    </div>
  );
}

function Message({ kind, children, onClose }: { kind: "error" | "success"; children: string; onClose: () => void }) {
  return <div className={`message ${kind}-message`} role={kind === "error" ? "alert" : "status"}>{kind === "error" ? <AlertCircle size={16} /> : <Check size={16} />}<span>{children}</span><button aria-label="Dismiss message" onClick={onClose} type="button"><X size={15} /></button></div>;
}

function toggleSetValue(current: Set<string>, value: string): Set<string> {
  const next = new Set(current);
  if (next.has(value)) next.delete(value);
  else next.add(value);
  return next;
}

function viewTitle(view: View): string {
  return { user: "User variables", system: "System variables", backups: "Backups" }[view];
}

function viewSubtitle(view: View): string {
  return { user: "HKCU\\Environment", system: "HKLM\\...\\Session Manager\\Environment", backups: "Automatic restore points" }[view];
}

function backupReason(reason: string): string {
  return { beforeSet: "Before save", beforeDelete: "Before delete", beforeRestore: "Before restore" }[reason] ?? reason;
}

export default App;
