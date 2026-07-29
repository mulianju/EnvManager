import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  AlertCircle,
  Check,
  DatabaseBackup,
  Eye,
  EyeOff,
  FileDown,
  FileUp,
  History,
  LoaderCircle,
  PanelLeftClose,
  PanelLeftOpen,
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
  SquareTerminal,
  X,
} from "lucide-react";
import "./App.css";
import envManagerMark from "./assets/envmanager-mark.svg";
import { PathEditor } from "./components/PathEditor";
import { TablePagination, useTablePagination } from "./components/TablePagination";
import { TransferDialog, type TransferDialogMode } from "./components/TransferDialog";
import { EffectiveVariablesView, VariablesView } from "./components/VariableViews";
import { CommandShimEditor } from "./components/CommandShimEditor";
import { CommandShimsView } from "./components/CommandShimsView";
import {
  apiErrorCode,
  apiErrorMessage,
  copyText,
  deleteCommandShim,
  deleteEnvironmentVariable,
  desktopErrorMessage,
  getEnvironmentRevision,
  getEnvironmentSnapshot,
  getFavorites,
  getCommandShims,
  repairCommandShims,
  restartElevated,
  restoreEnvironmentBackup,
  saveEnvironmentVariable,
  saveCommandShim,
  toggleFavorite,
  transferEnvironmentVariable,
  undoEnvironmentMutation,
} from "./lib/api";
import {
  canEditPath,
  canTransferVariable,
  createPathEntryDrafts,
  createPathEntryIdFactory,
  filterEffectiveVariables,
  filterVariables,
  formatVariableForCopy,
  finalizePathEdit,
  isPathVariable,
  isSensitiveVariable,
  parsePathEntries,
  revisionRefreshDecision,
  retryTransferAfterCollision,
  shouldApplyGeneration,
  transferConfirmationMessage,
} from "./lib/environment";
import { emptyCommandShimInput, filterCommandShims } from "./lib/command-shims";
import type {
  ApiError,
  BackupSummary,
  CommandShim,
  CommandShimInput,
  CommandShimSnapshot,
  EnvironmentScope,
  EnvironmentSnapshot,
  EnvironmentVariable,
  EnvironmentVariableInput,
  FavoriteKey,
  MutationResult,
  TransferMode,
  VariableCopyFormat,
} from "./types";

type View = EnvironmentScope | "effective" | "commandShims" | "backups";
interface NoticeState {
  message: string;
  undoBackupIds?: string[];
  revision?: string;
}
interface EditorSession {
  input: EnvironmentVariableInput;
  expectedRevision: string;
}

const navigation: Array<{ id: View; label: string; icon: typeof User }> = [
  { id: "user", label: "User variables", icon: User },
  { id: "system", label: "System variables", icon: Shield },
  { id: "effective", label: "Effective", icon: Variable },
  { id: "commandShims", label: "Command Shims", icon: SquareTerminal },
  { id: "backups", label: "Backups", icon: History },
];

function App() {
  const [view, setView] = useState<View>("user");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [snapshot, setSnapshot] = useState<EnvironmentSnapshot | null>(null);
  const [favorites, setFavorites] = useState<FavoriteKey[]>([]);
  const [commandShimSnapshot, setCommandShimSnapshot] = useState<CommandShimSnapshot | null>(null);
  const [commandShimLoading, setCommandShimLoading] = useState(true);
  const [commandShimError, setCommandShimError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [editor, setEditor] = useState<EditorSession | null>(null);
  const [commandShimEditor, setCommandShimEditor] = useState<CommandShimInput | null>(null);
  const [transferMode, setTransferMode] = useState<TransferDialogMode | null>(null);
  const [activeMenuKey, setActiveMenuKey] = useState<string | null>(null);
  const [deferredRevision, setDeferredRevision] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<NoticeState | null>(null);
  const requestId = useRef(0);
  const commandShimRequestId = useRef(0);
  const favoriteGeneration = useRef(0);
  const importButtonRef = useRef<HTMLButtonElement | null>(null);
  const exportButtonRef = useRef<HTMLButtonElement | null>(null);
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
      if (requestId.current !== currentRequest) return;
      commitSnapshot(next);
      setDeferredRevision(null);
      const currentCommandShimRequest = ++commandShimRequestId.current;
      setCommandShimLoading(true);
      try {
        const nextCommandShims = await getCommandShims();
        if (commandShimRequestId.current === currentCommandShimRequest) {
          setCommandShimSnapshot(nextCommandShims);
          setCommandShimError(null);
        }
      } catch (commandShimError) {
        if (commandShimRequestId.current === currentCommandShimRequest) {
          const message = apiErrorMessage(commandShimError);
          setCommandShimError(message);
          setError(`Environment loaded, but Command Shims could not be read: ${message}`);
        }
      } finally {
        if (commandShimRequestId.current === currentCommandShimRequest) {
          setCommandShimLoading(false);
        }
      }
      const favoritesRequest = ++favoriteGeneration.current;
      try {
        const nextFavorites = await getFavorites();
        if (shouldApplyGeneration(favoritesRequest, favoriteGeneration.current)) {
          setFavorites(nextFavorites);
        }
      } catch (favoriteError) {
        if (shouldApplyGeneration(favoritesRequest, favoriteGeneration.current)) {
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

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let mounted = true;
    let unlisten: (() => void) | undefined;
    void listen<ApiError>("desktop-error", (event) => {
      if (mounted) setError(desktopErrorMessage(event.payload));
    }).then((removeListener) => {
      if (mounted) unlisten = removeListener;
      else removeListener();
    }).catch((listenError) => {
      if (mounted) setError(apiErrorMessage(listenError));
    });
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, []);

  const interactionOpen = Boolean(editor || commandShimEditor || activeMenuKey || transferMode);

  const closeTransferDialog = () => {
    const trigger = transferMode === "import"
      ? importButtonRef.current
      : exportButtonRef.current;
    setTransferMode(null);
    window.requestAnimationFrame(() => trigger?.focus());
  };

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
        if (observedRevision !== current.revision) {
          setNotice((currentNotice) =>
            currentNotice?.revision && currentNotice.revision !== observedRevision
              ? null
              : currentNotice
          );
        }
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
  const commandShims = useMemo(
    () => filterCommandShims(commandShimSnapshot?.items ?? [], query),
    [commandShimSnapshot, query],
  );

  const reconcileFavorites = async () => {
    const generation = ++favoriteGeneration.current;
    try {
      const next = await getFavorites();
      if (shouldApplyGeneration(generation, favoriteGeneration.current)) {
        setFavorites(next);
      }
    } catch (nextError) {
      if (shouldApplyGeneration(generation, favoriteGeneration.current)) {
        setError(apiErrorMessage(nextError));
      }
    }
  };

  const acceptMutation = (result: MutationResult, message: string) => {
    requestId.current += 1;
    setLoading(false);
    commitSnapshot(result.snapshot);
    setDeferredRevision(null);
    setNotice({
      message,
      undoBackupIds: result.undoBackupIds,
      revision: result.snapshot.revision,
    });
    void reconcileFavorites();
  };

  const openVariable = (variable: EnvironmentVariable) => {
    if (!snapshot) return;
    setEditor({
      input: {
        originalName: variable.name,
        name: variable.name,
        value: variable.value,
        valueType: variable.valueType,
        scope: variable.scope,
      },
      expectedRevision: snapshot.revision,
    });
  };

  const addVariable = () => {
    if (!scope) return;
    if (!snapshot) return;
    setEditor({
      input: {
        originalName: null,
        name: "",
        value: "",
        valueType: "string",
        scope,
      },
      expectedRevision: snapshot.revision,
    });
  };

  const saveVariable = async (
    input: EnvironmentVariableInput,
    expectedRevision: string,
  ) => {
    if (deferredRevision) {
      setError("Environment values changed. Refresh before saving this edit.");
      return;
    }
    const next = await perform(() => saveEnvironmentVariable(input, expectedRevision));
    if (next) {
      acceptMutation(next, input.originalName ? "Variable updated." : "Variable created.");
      setEditor(null);
    }
  };

  const deleteVariable = async (
    input: EnvironmentVariableInput,
    expectedRevision: string,
  ) => {
    if (!input.originalName) {
      setEditor(null);
      return;
    }
    if (deferredRevision) {
      setError("Environment values changed. Refresh before deleting this variable.");
      return;
    }
    if (!window.confirm(`Delete ${input.originalName} from ${input.scope} variables?`)) return;
    const next = await perform(() =>
      deleteEnvironmentVariable(
        input.scope,
        input.originalName ?? input.name,
        expectedRevision,
      ),
    );
    if (next) {
      acceptMutation(next, "Variable deleted. A backup was created.");
      setEditor(null);
    }
  };

  const openCommandShim = (item: CommandShim) => {
    setCommandShimEditor({
      id: item.id,
      commandName: item.commandName,
      executable: item.executable,
      fixedArguments: [...item.fixedArguments],
    });
  };

  const saveEditedCommandShim = async (input: CommandShimInput) => {
    const next = await perform(() => saveCommandShim(input));
    if (!next) return;
    setCommandShimSnapshot(next);
    setCommandShimEditor(null);
    setNotice({
      message: input.id
        ? `${input.commandName.trim()} updated. Open a new terminal if PATH changed.`
        : `${input.commandName.trim()} created. Open a new terminal to use it.`,
    });
  };

  const deleteEditedCommandShim = async (input: CommandShimInput) => {
    if (!input.id) {
      setCommandShimEditor(null);
      return;
    }
    if (!window.confirm(`Delete the ${input.commandName} Command Shim? The target files will not be removed.`)) return;
    const next = await perform(() => deleteCommandShim(input.id!));
    if (!next) return;
    setCommandShimSnapshot(next);
    setCommandShimEditor(null);
    setNotice({ message: `${input.commandName} deleted. Its executable and fixed argument files were not changed.` });
  };

  const repairShellAccess = async () => {
    const next = await perform(() => repairCommandShims());
    if (!next) return;
    setCommandShimSnapshot(next);
    setNotice({
      message: "Shell access repaired. Fully quit and reopen your terminal or IDE before using these commands.",
    });
  };

  const undoMutation = async (backupIds: string[], expectedRevision: string) => {
    const next = await perform(() =>
      undoEnvironmentMutation(backupIds, expectedRevision),
    );
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
    const generation = ++favoriteGeneration.current;
    const next = await perform(() =>
      toggleFavorite({ scope: variable.scope, name: variable.name }),
    );
    if (next && shouldApplyGeneration(generation, favoriteGeneration.current)) {
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
    const expectedRevision = snapshot.revision;
    const input = {
      sourceScope: variable.scope,
      targetScope,
      name: variable.name,
      mode,
      overwrite: false,
    };
    const confirmation = transferConfirmationMessage(input);
    if (confirmation && !window.confirm(confirmation)) return;
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setNotice(null);
    let next: MutationResult | null = null;
    try {
      try {
        next = await transferEnvironmentVariable(input, expectedRevision);
      } catch (transferError) {
        const retry = retryTransferAfterCollision(
          { input, expectedRevision },
          apiErrorCode(transferError),
        );
        if (!retry) throw transferError;
        const overwriteConfirmation = transferConfirmationMessage(retry.input);
        if (!overwriteConfirmation || !window.confirm(overwriteConfirmation)) return;
        next = await transferEnvironmentVariable(retry.input, retry.expectedRevision);
      }
    } catch (transferError) {
      setError(apiErrorMessage(transferError));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
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
      setCommandShimEditor(null);
      setActiveMenuKey(null);
    }
    await refresh();
  };

  const elevate = async () => {
    const result = await perform(restartElevated);
    if (result !== null) await refresh();
  };

  return (
    <div className={sidebarCollapsed ? "app-shell sidebar-collapsed" : "app-shell"}>
      <aside className={sidebarCollapsed ? "sidebar collapsed" : "sidebar"}>
        <div className="brand">
          <span className="brand-mark"><img alt="" aria-hidden="true" src={envManagerMark} /></span>
          <span className="brand-name">EnvManager</span>
        </div>
        <nav className="nav-list" id="primary-navigation" aria-label="Primary navigation">
          {navigation.map((item) => {
            const Icon = item.icon;
            const count = snapshot
              ? item.id === "user"
                ? snapshot.userVariables.length
                : item.id === "system"
                  ? snapshot.systemVariables.length
                  : item.id === "effective"
                    ? snapshot.effectiveVariables.length
                    : item.id === "commandShims"
                      ? commandShimSnapshot?.items.length ?? 0
                      : snapshot.backups.length
              : 0;
            return (
              <button
                aria-label={item.label}
                aria-current={view === item.id ? "page" : undefined}
                className={view === item.id ? "nav-item active" : "nav-item"}
                key={item.id}
                onClick={() => {
                  setView(item.id);
                  setQuery("");
                  setActiveMenuKey(null);
                }}
                title={sidebarCollapsed ? item.label : undefined}
                type="button"
              >
                <Icon size={17} />
                <span className="nav-item-label">{item.label}</span>
                <small>{count}</small>
              </button>
            );
          })}
        </nav>
        <div className="sidebar-footer">
          <button
            aria-controls="primary-navigation"
            aria-expanded={!sidebarCollapsed}
            aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
            className="sidebar-toggle"
            onClick={() => setSidebarCollapsed((current) => !current)}
            title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
            type="button"
          >
            {sidebarCollapsed ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}
          </button>
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
                <input aria-label={view === "commandShims" ? "Search Command Shims" : "Search variables"} placeholder="Search" value={query} onChange={(event) => setQuery(event.target.value)} />
              </label>
            )}
            {view !== "commandShims" && <div className="transfer-actions" role="group" aria-label="Import and export">
              <button
                className="secondary-button"
                disabled={loading || busy || !snapshot}
                onClick={() => {
                  setActiveMenuKey(null);
                  setTransferMode("import");
                }}
                ref={importButtonRef}
                type="button"
              >
                <FileUp size={15} /> Import
              </button>
              <button
                className="secondary-button"
                disabled={loading || busy || !snapshot}
                onClick={() => {
                  setActiveMenuKey(null);
                  setTransferMode("export");
                }}
                ref={exportButtonRef}
                type="button"
              >
                <FileDown size={15} /> Export
              </button>
            </div>}
            <button className="icon-button" disabled={loading || busy} onClick={() => void requestRefresh()} title="Refresh" type="button">
              <RefreshCw className={loading ? "spin" : ""} size={17} />
            </button>
            {scope && (
              <button className="primary-button" disabled={scope === "system" && !snapshot?.isElevated} onClick={addVariable} type="button">
                <Plus size={16} /> Add variable
              </button>
            )}
            {view === "commandShims" && (
              <button className="primary-button" disabled={busy || !commandShimSnapshot} onClick={() => setCommandShimEditor(emptyCommandShimInput())} type="button">
                <Plus size={16} /> Add command
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
              ? () => void undoMutation(notice.undoBackupIds!, notice.revision!)
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
                paginationKey={`${scope}:${query}`}
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
            ) : view === "commandShims" ? (
              commandShimSnapshot ? (
                <CommandShimsView
                  busy={busy}
                  items={commandShims}
                  onEdit={openCommandShim}
                  onRepair={() => void repairShellAccess()}
                  query={query}
                  snapshot={commandShimSnapshot}
                />
              ) : commandShimLoading ? (
                <div className="loading-state"><LoaderCircle className="spin" size={24} /> Loading Command Shims</div>
              ) : (
                <div className="loading-state error-state" role="alert">
                  <AlertCircle size={24} />
                  <strong>Command Shims could not be loaded</strong>
                  <span>{commandShimError}</span>
                  <button className="secondary-button" onClick={() => void requestRefresh()} type="button">
                    <RefreshCw size={15} /> Retry
                  </button>
                </div>
              )
            ) : (
              <BackupsView
                busy={busy}
                onRestore={async (backup) => {
                  if (backup.scope === "system" && !snapshot.isElevated) {
                    setError("Restart as administrator before restoring a system backup.");
                    return;
                  }
                  if (!window.confirm(`Restore this ${backup.scope} backup? Current values in that scope will be replaced.`)) return;
                  const next = await perform(() =>
                    restoreEnvironmentBackup(backup.id, snapshot.revision)
                  );
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
          input={editor.input}
          isElevated={snapshot?.isElevated ?? false}
          onClose={() => setEditor(null)}
          onDelete={() => void deleteVariable(editor.input, editor.expectedRevision)}
          onError={setError}
          onSave={(input) => void saveVariable(input, editor.expectedRevision)}
        />
      )}
      {commandShimEditor && (
        <CommandShimEditor
          busy={busy}
          input={commandShimEditor}
          onClose={() => setCommandShimEditor(null)}
          onDelete={() => void deleteEditedCommandShim(commandShimEditor)}
          onError={setError}
          onSave={(input) => void saveEditedCommandShim(input)}
        />
      )}
      {transferMode && snapshot && (
        <TransferDialog
          isElevated={snapshot.isElevated}
          mode={transferMode}
          onClose={closeTransferDialog}
          onImported={acceptMutation}
          onNotice={(message) => {
            setError(null);
            setNotice({ message });
          }}
          onRestartElevated={async () => {
            await restartElevated();
            await refresh();
          }}
        />
      )}
    </div>
  );
}

function BackupsView({ snapshot, busy, onRestore }: { snapshot: EnvironmentSnapshot; busy: boolean; onRestore: (backup: BackupSummary) => void }) {
  const pagination = useTablePagination(snapshot.backups, "backups");
  return (
    <section className="content-section">
      <div className="backup-location"><DatabaseBackup size={17} /><span>{snapshot.backupDirectory}</span></div>
      <div className="table-shell">
        <div className="backup-table">
          <div className="backup-header"><span>Created</span><span>Scope</span><span>Reason</span><span>Variables</span><span /></div>
          {pagination.items.map((backup) => (
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
        <TablePagination
          {...pagination}
          onPageChange={pagination.setPage}
          onPageSizeChange={pagination.setPageSize}
        />
      </div>
    </section>
  );
}

function VariableEditor({ input, busy, isElevated, onSave, onDelete, onClose, onError }: { input: EnvironmentVariableInput; busy: boolean; isElevated: boolean; onSave: (input: EnvironmentVariableInput) => void; onDelete: () => void; onClose: () => void; onError: (message: string) => void }) {
  const [draft, setDraft] = useState(input);
  const [createPathEntryId] = useState(() => createPathEntryIdFactory());
  const [pathDrafts, setPathDrafts] = useState(() =>
    createPathEntryDrafts(
      parsePathEntries(input.value),
      createPathEntryId,
    )
  );
  const [showSensitive, setShowSensitive] = useState(false);
  const pathMode = isPathVariable(draft.name) || isPathVariable(input.originalName ?? "");
  const canMutate = canEditPath(draft.scope, isElevated, busy);

  const submit = () => {
    const value = pathMode
      ? finalizePathEdit(input.value, pathDrafts.map((entry) => entry.value)).value
      : draft.value;
    onSave({ ...draft, value });
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
            <label><span>Name</span><input autoFocus disabled={!canMutate} value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
            <label><span>Registry type</span><select disabled={!canMutate} value={draft.valueType} onChange={(event) => setDraft({ ...draft, valueType: event.target.value as EnvironmentVariableInput["valueType"] })}><option value="string">String (REG_SZ)</option><option value="expandableString">Expandable string (REG_EXPAND_SZ)</option></select></label>
          </div>
          {pathMode ? (
            <PathEditor
              busy={busy}
              createId={createPathEntryId}
              drafts={pathDrafts}
              isElevated={isElevated}
              onChange={setPathDrafts}
              onError={onError}
              originalRaw={input.value}
              scope={draft.scope}
            />
          ) : (
            <label className="value-field">
              <span>Value</span>
              <div className="value-input-wrap">
                <textarea className={isSensitiveVariable(draft.name) && !showSensitive ? "secret-masked" : ""} disabled={!canMutate} rows={7} value={draft.value} onChange={(event) => setDraft({ ...draft, value: event.target.value })} />
                {isSensitiveVariable(draft.name) && <button aria-label={showSensitive ? "Hide value" : "Show value"} className="icon-button small reveal-button" onClick={() => setShowSensitive((current) => !current)} type="button">{showSensitive ? <EyeOff size={15} /> : <Eye size={15} />}</button>}
              </div>
            </label>
          )}
        </div>
        <footer className="modal-footer">
          <div>{input.originalName && <button className="danger-button" disabled={!canMutate} onClick={onDelete} type="button"><Trash2 size={16} /> Delete</button>}</div>
          <div className="button-row"><button className="secondary-button" onClick={onClose} type="button">Cancel</button><button className="primary-button" disabled={!canMutate || !draft.name.trim()} onClick={submit} type="button">{busy ? <LoaderCircle className="spin" size={16} /> : <Save size={16} />} Save</button></div>
        </footer>
      </section>
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
      favorite.name === variable.name,
  );
}

function viewTitle(view: View): string {
  return { user: "User variables", system: "System variables", effective: "Effective environment", commandShims: "Command Shims", backups: "Backups" }[view];
}

function viewSubtitle(view: View): string {
  return { user: "HKCU\\Environment", system: "HKLM\\...\\Session Manager\\Environment", effective: "Environment inherited by newly launched processes", commandShims: "User commands backed by managed .cmd files", backups: "Automatic restore points" }[view];
}

function backupReason(reason: string): string {
  return { beforeSet: "Before save", beforeDelete: "Before delete", beforeRestore: "Before restore" }[reason] ?? reason;
}

export default App;
