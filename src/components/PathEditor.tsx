import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent as ReactDragEvent,
} from "react";
import {
  AlertCircle,
  ArrowDown,
  ArrowUp,
  CircleHelp,
  ClipboardPaste,
  FolderCheck,
  FolderOpen,
  FolderX,
  GripVertical,
  LoaderCircle,
  Plus,
  Trash2,
} from "lucide-react";
import {
  analyzePathEntries,
  apiErrorMessage,
  pickEnvironmentFolder,
} from "../lib/api";
import {
  canEditPath,
  insertPathEntries,
  PATH_ENTRY_DRAG_MIME,
  parsePathBulkInput,
  parsePathDragIndex,
  pathFilterCounts,
  removeDuplicatePathEntries,
  reorderPathEntries,
  summarizePathChanges,
} from "../lib/environment";
import type {
  EnvironmentScope,
  PathEntryStatus,
  PathStatusFilter,
} from "../types";

interface PathEditorProps {
  busy: boolean;
  entries: string[];
  isElevated: boolean;
  onChange: (entries: string[]) => void;
  onError: (message: string) => void;
  originalEntries: string[];
  scope: EnvironmentScope;
}

export function PathEditor({
  busy,
  entries,
  isElevated,
  onChange,
  onError,
  originalEntries,
  scope,
}: PathEditorProps) {
  const [statuses, setStatuses] = useState<PathEntryStatus[]>([]);
  const [checking, setChecking] = useState(false);
  const [bulkInput, setBulkInput] = useState("");
  const [feedback, setFeedback] = useState<string | null>(null);
  const [analysisError, setAnalysisError] = useState<string | null>(null);
  const [pickerError, setPickerError] = useState<string | null>(null);
  const [filter, setFilter] = useState<PathStatusFilter>("all");
  const [pickingFolder, setPickingFolder] = useState(false);
  const [draggingIndex, setDraggingIndex] = useState<number | null>(null);
  const analysisGeneration = useRef(0);
  const disabled = !canEditPath(scope, isElevated, busy) || pickingFolder;
  const currentStatuses =
    statuses.length === entries.length &&
    statuses.every((status, index) => status.value === entries[index])
      ? statuses
      : [];
  const analyzedCounts = pathFilterCounts(currentStatuses);
  const counts = { ...analyzedCounts, all: entries.length };
  const changes = useMemo(
    () => summarizePathChanges(originalEntries, entries),
    [entries, originalEntries],
  );
  const hasChanges =
    changes.added.length > 0 ||
    changes.removed.length > 0 ||
    changes.orderChanged;
  const visibleEntries = entries
    .map((entry, index) => ({ entry, index, status: currentStatuses[index] }))
    .filter(({ status }) =>
      filter === "all" ||
      (filter === "duplicate" && status?.duplicate) ||
      (filter === "missing" && status && !status.exists)
    );

  useEffect(() => {
    const generation = ++analysisGeneration.current;
    let cancelled = false;
    setStatuses([]);
    setAnalysisError(null);
    setChecking(true);
    const timeout = window.setTimeout(() => {
      void analyzePathEntries(entries)
        .then((nextStatuses) => {
          if (!cancelled && generation === analysisGeneration.current) {
            setStatuses(nextStatuses);
          }
        })
        .catch((error) => {
          if (!cancelled && generation === analysisGeneration.current) {
            const message = `PATH analysis failed: ${apiErrorMessage(error)}`;
            setStatuses([]);
            setAnalysisError(message);
            onError(message);
          }
        })
        .finally(() => {
          if (!cancelled && generation === analysisGeneration.current) {
            setChecking(false);
          }
        });
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timeout);
    };
  }, [entries, onError]);

  const update = (index: number, value: string) => {
    onChange(entries.map((entry, current) => current === index ? value : entry));
  };
  const move = (index: number, direction: -1 | 1) => {
    onChange(reorderPathEntries(entries, index, index + direction));
  };
  const insertBulkEntries = () => {
    const parsedEntries = parsePathBulkInput(bulkInput);
    if (parsedEntries.length === 0) {
      setFeedback("Enter one or more PATH entries first.");
      return;
    }
    const nextEntries = insertPathEntries(entries, bulkInput);
    const addedCount = nextEntries.length - entries.length;
    const skippedCount = parsedEntries.length - addedCount;
    if (addedCount === 0) {
      setFeedback("No entries added. Every pasted path is already present.");
      return;
    }
    onChange(nextEntries);
    setBulkInput("");
    setFeedback(`Added ${addedCount} ${addedCount === 1 ? "entry" : "entries"}.${skippedCount > 0 ? ` Skipped ${skippedCount} duplicate${skippedCount === 1 ? "" : "s"}.` : ""}`);
  };
  const addFolder = async () => {
    if (disabled) return;
    setPickingFolder(true);
    setFeedback(null);
    setPickerError(null);
    try {
      const folder = await pickEnvironmentFolder();
      if (!folder) {
        setFeedback("Folder selection canceled. PATH unchanged.");
        return;
      }
      const nextEntries = insertPathEntries(entries, folder);
      if (nextEntries === entries) {
        setFeedback("That folder is already in PATH. No changes made.");
        return;
      }
      onChange(nextEntries);
      setFeedback("Folder added to PATH.");
    } catch (error) {
      const message = `Folder picker failed: ${apiErrorMessage(error)}`;
      setPickerError(message);
      onError(message);
    } finally {
      setPickingFolder(false);
    }
  };
  const removeDuplicates = () => {
    const nextEntries = removeDuplicatePathEntries(entries);
    const removedCount = entries.length - nextEntries.length;
    if (removedCount === 0) {
      setFeedback("No duplicate entries to remove.");
      return;
    }
    onChange(nextEntries);
    setFeedback(`Removed ${removedCount} duplicate ${removedCount === 1 ? "entry" : "entries"}. Missing paths were kept.`);
  };
  const startDrag = (event: ReactDragEvent<HTMLElement>, index: number) => {
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData(PATH_ENTRY_DRAG_MIME, String(index));
    setDraggingIndex(index);
  };
  const dropEntry = (event: ReactDragEvent<HTMLDivElement>, targetIndex: number) => {
    event.preventDefault();
    const sourceIndex = parsePathDragIndex(
      event.dataTransfer.getData(PATH_ENTRY_DRAG_MIME),
      entries.length,
      draggingIndex,
    );
    if (sourceIndex !== null) {
      onChange(reorderPathEntries(entries, sourceIndex, targetIndex));
    }
    setDraggingIndex(null);
  };

  return (
    <>
      <div className="path-editor">
        <div className="path-toolbar">
          <div><h3>PATH entries</h3><span>{entries.length} ordered entries</span></div>
          <div className="path-toolbar-actions">
            <button className="secondary-button" disabled={disabled} onClick={() => void addFolder()} type="button"><FolderOpen size={15} /> Choose folder</button>
            <button className="secondary-button" disabled={disabled} onClick={() => onChange([...entries, ""])} type="button"><Plus size={15} /> Add entry</button>
          </div>
        </div>
        <div className="path-bulk-input">
          <label><span>Paste multiple entries</span><textarea disabled={disabled} placeholder={"C:\\Tools;D:\\Apps\n%JAVA_HOME%\\bin"} rows={3} value={bulkInput} onChange={(event) => setBulkInput(event.target.value)} /></label>
          <button className="secondary-button" disabled={disabled || !bulkInput.trim()} onClick={insertBulkEntries} type="button"><ClipboardPaste size={15} /> Insert entries</button>
        </div>
        {feedback && <div className="path-feedback" role="status">{feedback}</div>}
        {(analysisError || pickerError) && (
          <div className="path-local-errors">
            {analysisError && <div role="alert"><AlertCircle size={15} /><span>{analysisError}</span></div>}
            {pickerError && <div role="alert"><AlertCircle size={15} /><span>{pickerError}</span></div>}
          </div>
        )}
        <div className="path-list-controls">
          <div aria-label="Filter PATH entries" className="path-filters" role="group">
            {(["all", "duplicate", "missing"] as PathStatusFilter[]).map((option) => (
              <button aria-pressed={filter === option} className={filter === option ? "active" : ""} key={option} onClick={() => setFilter(option)} type="button">
                {option === "all" ? "All" : option === "duplicate" ? "Duplicate" : "Missing"} <span>{counts[option]}</span>
              </button>
            ))}
          </div>
          <button className="secondary-button compact-button" disabled={disabled || checking || analyzedCounts.duplicate === 0} onClick={removeDuplicates} type="button"><Trash2 size={14} /> Remove duplicates</button>
        </div>
        <div className="path-list">
          {visibleEntries.map(({ entry, index, status }) => {
            const statusLabel = status
              ? status.duplicate
                ? "Duplicate entry"
                : status.exists
                  ? "Path exists"
                  : "Path not found"
              : checking
                ? "Checking path..."
                : analysisError
                  ? "Path status unavailable"
                  : "Path status unknown";
            const statusClass = status
              ? status.duplicate || !status.exists ? "warning" : "valid"
              : "unknown";
            const statusIcon = checking && !status
              ? <LoaderCircle className="spin" size={15} />
              : status
                ? status.exists && !status.duplicate
                  ? <FolderCheck size={16} />
                  : <FolderX size={16} />
                : <CircleHelp size={16} />;
            return (
              <div
                className={`path-row${status?.duplicate ? " duplicate" : ""}${status && !status.exists ? " missing" : ""}${draggingIndex === index ? " dragging" : ""}`}
                key={index}
                onDragEnd={() => setDraggingIndex(null)}
                onDragOver={(event) => { if (!disabled) event.preventDefault(); }}
                onDrop={(event) => { if (!disabled) dropEntry(event, index); }}
              >
                <span className="path-drag-handle" draggable={!disabled} onDragStart={(event) => startDrag(event, index)} title="Drag to reorder"><GripVertical size={15} /></span>
                <span className="path-index">{index + 1}</span>
                <div className="path-input"><input aria-label={`PATH entry ${index + 1}`} disabled={disabled} value={entry} onChange={(event) => update(index, event.target.value)} /><small title={status?.expandedValue}>{statusLabel}</small></div>
                <span className={`path-status ${statusClass}`}>{statusIcon}</span>
                <div className="path-actions"><button aria-label="Move up" className="icon-button small" disabled={disabled || index === 0} onClick={() => move(index, -1)} type="button"><ArrowUp size={14} /></button><button aria-label="Move down" className="icon-button small" disabled={disabled || index === entries.length - 1} onClick={() => move(index, 1)} type="button"><ArrowDown size={14} /></button><button aria-label="Remove entry" className="icon-button small danger-icon" disabled={disabled} onClick={() => onChange(entries.filter((_, current) => current !== index))} type="button"><Trash2 size={14} /></button></div>
              </div>
            );
          })}
          {visibleEntries.length === 0 && <div className="table-empty">{filter !== "all" && currentStatuses.length === 0 ? checking ? "Checking paths..." : analysisError ? "Path status unavailable" : "Path status unknown" : entries.length === 0 ? "No PATH entries" : `No ${filter} entries`}</div>}
        </div>
      </div>
      <div className="path-change-summary" role="status">
        <h3>Changes before save</h3>
        {!hasChanges ? (
          <p>No PATH changes.</p>
        ) : (
          <div className="path-change-groups">
            {changes.added.length > 0 && <PathChangeGroup label="Added" values={changes.added} />}
            {changes.removed.length > 0 && <PathChangeGroup label="Removed" values={changes.removed} />}
            {changes.moved.length > 0 && <PathChangeGroup label="Moved" values={changes.moved.map(({ value, fromIndex, toIndex }) => `${value}: #${fromIndex + 1} -> #${toIndex + 1}`)} />}
          </div>
        )}
      </div>
    </>
  );
}

function PathChangeGroup({ label, values }: { label: string; values: string[] }) {
  return (
    <div className="path-change-group">
      <strong>{label} ({values.length})</strong>
      <ul>{values.map((value, index) => <li key={`${value}:${index}`}>{value}</li>)}</ul>
    </div>
  );
}
