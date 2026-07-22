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
  Plus,
  RefreshCw,
  Save,
  Search,
  Shield,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  Undo2,
  User,
  Variable,
  X,
} from "lucide-react";
import "./App.css";
import { EffectiveVariablesView, VariablesView } from "./components/VariableViews";
import {
  analyzePathEntries,
  apiErrorMessage,
  copyText,
  deleteEnvironmentVariable,
  getEnvironmentRevision,
  getEnvironmentSnapshot,
  getFavorites,
  restartElevated,
  restoreEnvironmentBackup,
  saveEnvironmentVariable,
  toggleFavorite,
  transferEnvironmentVariable,
  undoEnvironmentMutation,
} from "./lib/api";
import {
  canTransferVariable,
  filterEffectiveVariables,
  filterVariables,
  formatVariableForCopy,
  isPathVariable,
  isSensitiveVariable,
  joinPathEntries,
  parsePathEntries,
  revisionRefreshDecision,
  transferConfirmationMessage,
} from "./lib/environment";
import type {
  BackupSummary,
  EnvironmentScope,
  EnvironmentSnapshot,
  EnvironmentVariable,
  EnvironmentVariableInput,
  FavoriteKey,
  MutationResult,
  PathEntryStatus,
  TransferMode,
  VariableCopyFormat,
} from "./types";

type View = EnvironmentScope | "effective" | "backups";
interface NoticeState {
  message: string;
  undoBackupIds?: string[];
}

const navigation: Array<{ id: View; label: string; icon: typeof User }> = [
  { id: "user", label: "User variables", icon: User },
  { id: "system", label: "System variables", icon: Shield },
  { id: "effective", label: "Effective", icon: Variable },
  { id: "backups", label: "Backups", icon: History },
];

function App() {
  const [view, setView] = useState<View>("user");
  const [snapshot, setSnapshot] = useState<EnvironmentSnapshot | null>(null);
  const [favorites, setFavorites] = useState<FavoriteKey[]>([]);
  const [query, setQuery] = useState("");
  const [editor, setEditor] = useState<EnvironmentVariableInput | null>(null);
  const [activeMenuKey, setActiveMenuKey] = useState<string | null>(null);
  const [deferredRevision, setDeferredRevision] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<NoticeState | null>(null);
  const requestId = useRef(0);
  const snapshotRef = useRef<EnvironmentSnapshot | null>(null);
  const interactionOpenRef = useRef(false);
  const busyRef = useRef(false);

  const commitSnapshot = useCallback((next: EnvironmentSnapshot) => {
    snapshotRef.current = next;
    setSnapshot(next);
  }, []);

  const refresh = useCallback(async (showLoading = true) => {
    const currentRequest = ++requestId.current;
    if (showLoading) setLoading(true);
    setError(null);
    try {
      const next = await getEnvironmentSnapshot();
      if (requestId.current === currentRequest) {
        commitSnapshot(next);
        setDeferredRevision(null);
      }
      try {
        const nextFavorites = await getFavorites();
        if (requestId.current === currentRequest) setFavorites(nextFavorites);
      } catch (favoriteError) {
        if (requestId.current === currentRequest) {
          setError(`Variables loaded, but favorites could not be read: ${apiErrorMessage(favoriteError)}`);
        }
      }
    } catch (nextError) {
      if (requestId.current === currentRequest) setError(apiErrorMessage(nextError));
    } finally {
      if (requestId.current === currentRequest) setLoading(false);
    }
  }, [commitSnapshot]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const interactionOpen = Boolean(editor || activeMenuKey);
  useEffect(() => {
    interactionOpenRef.current = interactionOpen;
  }, [interactionOpen]);

  useEffect(() => {
    if (!activeMenuKey) return;
    const closeMenu = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Element && target.closest(".actions-menu-wrap")) return;
      setActiveMenuKey(null);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setActiveMenuKey(null);
    };
    document.addEventListener("pointerdown", closeMenu);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeMenu);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [activeMenuKey]);

  useEffect(() => {
    const poll = async () => {
      const current = snapshotRef.current;
      if (!current || busyRef.current) return;
      try {
        const observedRevision = await getEnvironmentRevision();
        const decision = revisionRefreshDecision(
          current.revision,
          observedRevision,
          interactionOpenRef.current,
        );
        if (decision === "defer") {
          setDeferredRevision(observedRevision);
        } else if (decision === "refresh") {
          await refresh(false);
        }
      } catch (nextError) {
        setError(apiErrorMessage(nextError));
      }
    };
    const interval = window.setInterval(() => void poll(), 2000);
    return () => window.clearInterval(interval);
  }, [refresh]);

  const perform = async <T,>(action: () => Promise<T>): Promise<T | null> => {
    if (busyRef.current) return null;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      return await action();
    } catch (nextError) {
      setError(apiErrorMessage(nextError));
      return null;
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const scope = view === "user" || view === "system" ? view : null;
  const variables = useMemo(() => {
    if (!snapshot || !scope) return [];
    return filterVariables(
      scope === "user" ? snapshot.userVariables : snapshot.systemVariables,
      query,
    );
  }, [snapshot, scope, query]);
  const effectiveVariables = useMemo(
    () => filterEffectiveVariables(snapshot?.effectiveVariables ?? [], query),
    [snapshot, query],
  );

  const reconcileFavorites = async () => {
    try {
      setFavorites(await getFavorites());
    } catch (nextError) {
      setError(apiErrorMessage(nextError));
    }
  };

  const acceptMutation = (result: MutationResult, message: string) => {
    commitSnapshot(result.snapshot);
    setDeferredRevision(null);
    setNotice({ message, undoBackupIds: result.undoBackupIds });
    void reconcileFavorites();
  };

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
      acceptMutation(next, input.originalName ? "Variable updated." : "Variable created.");
      setEditor(null);
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
      acceptMutation(next, "Variable deleted. A backup was created.");
      setEditor(null);
    }
  };

  const undoMutation = async (backupIds: string[]) => {
    const next = await perform(() => undoEnvironmentMutation(backupIds));
    if (next) acceptMutation(next, "Change undone. You can undo this restore too.");
  };

  const copyVariable = async (
    variable: Pick<EnvironmentVariable, "name" | "value">,
    format: VariableCopyFormat,
  ) => {
    setActiveMenuKey(null);
    const result = await perform(() => copyText(formatVariableForCopy(variable, format)));
    if (result !== null) {
      const label = format === "powershell" ? "PowerShell reference" : format;
      setNotice({ message: `Copied ${label}.` });
    }
  };

  const toggleVariableFavorite = async (variable: EnvironmentVariable) => {
    setActiveMenuKey(null);
    const next = await perform(() =>
      toggleFavorite({ scope: variable.scope, name: variable.name }),
    );
    if (next) {
      setFavorites(next);
      setNotice({ message: isFavorite(favorites, variable) ? "Favorite removed." : "Favorite added." });
    }
  };

  const transferVariable = async (variable: EnvironmentVariable, mode: TransferMode) => {
    setActiveMenuKey(null);
    if (!snapshot || !canTransferVariable(variable.scope, mode, snapshot.isElevated)) {
      setError("Administrator permission is required for this transfer.");
      return;
    }
    const targetScope: EnvironmentScope = variable.scope === "user" ? "system" : "user";
    const targetVariables = targetScope === "user"
      ? snapshot.userVariables
      : snapshot.systemVariables;
    const overwrite = targetVariables.some(
      (item) => item.name.toLowerCase() === variable.name.toLowerCase(),
    );
    const input = {
      sourceScope: variable.scope,
      targetScope,
      name: variable.name,
      mode,
      overwrite,
    };
    const confirmation = transferConfirmationMessage(input);
    if (confirmation && !window.confirm(confirmation)) return;
    const next = await perform(() => transferEnvironmentVariable(input));
    if (next) {
      acceptMutation(
        next,
        `${variable.name} ${mode === "move" ? "moved" : "copied"} to ${targetScope} variables.`,
      );
    }
  };

  const requestRefresh = async () => {
    if (interactionOpenRef.current) {
      const confirmed = window.confirm(
        "Close the open editor or menu and discard unsaved changes before refreshing?",
      );
      if (!confirmed) return;
      setEditor(null);
      setActiveMenuKey(null);
    }
    await refresh();
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
                  : item.id === "effective"
                    ? snapshot.effectiveVariables.length
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
                  setActiveMenuKey(null);
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
            {view !== "backups" && (
              <label className="search-control">
                <Search size={16} />
                <input aria-label="Search variables" placeholder="Search" value={query} onChange={(event) => setQuery(event.target.value)} />
              </label>
            )}
            <button className="icon-button" disabled={loading || busy} onClick={() => void requestRefresh()} title="Refresh" type="button">
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
        {deferredRevision && (
          <Message
            actionLabel="Refresh"
            kind="refresh"
            onAction={() => void requestRefresh()}
            onClose={() => setDeferredRevision(null)}
          >
            Environment values changed outside EnvManager. Your open work was not replaced.
          </Message>
        )}
        {notice && (
          <Message
            actionLabel={notice.undoBackupIds?.length ? "Undo" : undefined}
            kind="success"
            onAction={notice.undoBackupIds?.length
              ? () => void undoMutation(notice.undoBackupIds!)
              : undefined}
            onClose={() => setNotice(null)}
          >
            {notice.message}
          </Message>
        )}

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
                activeMenuKey={activeMenuKey}
                busy={busy}
                canEdit={scope === "user" || snapshot.isElevated}
                favorites={favorites}
                isElevated={snapshot.isElevated}
                onCopy={copyVariable}
                onFavorite={toggleVariableFavorite}
                onMenuChange={setActiveMenuKey}
                onOpen={openVariable}
                onTransfer={transferVariable}
                query={query}
                variables={variables}
              />
            ) : view === "effective" ? (
              <EffectiveVariablesView
                busy={busy}
                onCopy={copyVariable}
                query={query}
                variables={effectiveVariables}
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
                    acceptMutation(next, "Backup restored. A rollback backup was created.");
                  }
                }}
                snapshot={snapshot}
              />
            )}
          </>
        ) : (
          <div className="empty-state"><AlertCircle size={26} /><strong>Unable to read environment variables</strong><button className="primary-button" onClick={() => void requestRefresh()} type="button"><RefreshCw size={16} /> Retry</button></div>
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

function Message({
  kind,
  children,
  actionLabel,
  onAction,
  onClose,
}: {
  kind: "error" | "success" | "refresh";
  children: string;
  actionLabel?: string;
  onAction?: () => void;
  onClose: () => void;
}) {
  const icon = kind === "error"
    ? <AlertCircle size={16} />
    : kind === "refresh"
      ? <RefreshCw size={16} />
      : <Check size={16} />;
  return (
    <div className={`message ${kind}-message`} role={kind === "error" ? "alert" : "status"}>
      {icon}
      <span>{children}</span>
      {actionLabel && onAction && (
        <button className="message-action" onClick={onAction} type="button">
          {actionLabel === "Undo" && <Undo2 size={14} />}{actionLabel}
        </button>
      )}
      <button aria-label="Dismiss message" onClick={onClose} type="button"><X size={15} /></button>
    </div>
  );
}

function isFavorite(
  favorites: FavoriteKey[],
  variable: Pick<EnvironmentVariable, "scope" | "name">,
): boolean {
  return favorites.some(
    (favorite) =>
      favorite.scope === variable.scope &&
      favorite.name.toLowerCase() === variable.name.toLowerCase(),
  );
}

function viewTitle(view: View): string {
  return { user: "User variables", system: "System variables", effective: "Effective environment", backups: "Backups" }[view];
}

function viewSubtitle(view: View): string {
  return { user: "HKCU\\Environment", system: "HKLM\\...\\Session Manager\\Environment", effective: "Environment inherited by newly launched processes", backups: "Automatic restore points" }[view];
}

function backupReason(reason: string): string {
  return { beforeSet: "Before save", beforeDelete: "Before delete", beforeRestore: "Before restore" }[reason] ?? reason;
}

export default App;
