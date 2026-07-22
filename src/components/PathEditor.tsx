import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent as ReactDragEvent,
} from "react";
import {
  AlertCircle,
  ClipboardPaste,
  FolderOpen,
  Plus,
  Trash2,
} from "lucide-react";
import {
  analyzePathEntries,
  apiErrorMessage,
  pickEnvironmentFolder,
} from "../lib/api";
import {
  appendPathEntryDraft,
  canEditPath,
  finalizePathEdit,
  insertPathEntryDrafts,
  PATH_ENTRY_DRAG_MIME,
  parsePathBulkInput,
  parsePathDragIndex,
  pathFilterCounts,
  removeDuplicatePathEntryDrafts,
  reorderPathEntries,
  updatePathEntryDrafts,
} from "../lib/environment";
import type {
  EnvironmentScope,
  PathEntryDraft,
  PathEntryStatus,
  PathStatusFilter,
} from "../types";
import { PathRow } from "./PathRow";

interface PathEditorProps {
  busy: boolean;
  createId: () => string;
  drafts: PathEntryDraft[];
  isElevated: boolean;
  onChange: (drafts: PathEntryDraft[]) => void;
  onError: (message: string) => void;
  originalRaw: string;
  scope: EnvironmentScope;
}

export function PathEditor({
  busy,
  createId,
  drafts,
  isElevated,
  onChange,
  onError,
  originalRaw,
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
  const entries = useMemo(() => drafts.map(({ value }) => value), [drafts]);
  const currentStatuses =
    statuses.length === entries.length &&
    statuses.every((status, index) => status.value === entries[index])
      ? statuses
      : [];
  const analyzedCounts = pathFilterCounts(currentStatuses);
  const counts = { ...analyzedCounts, all: entries.length };
  const changes = useMemo(
    () => finalizePathEdit(originalRaw, entries).summary,
    [entries, originalRaw],
  );
  const hasChanges =
    changes.added.length > 0 ||
    changes.removed.length > 0 ||
    changes.moved.length > 0;
  const visibleEntries = drafts
    .map((draft, index) => ({ draft, index, status: currentStatuses[index] }))
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

  const update = (id: string, value: string) => {
    onChange(updatePathEntryDrafts(drafts, id, value));
  };
  const move = (index: number, direction: -1 | 1) => {
    onChange(reorderPathEntries(drafts, index, index + direction));
  };
  const insertBulkEntries = () => {
    const parsedEntries = parsePathBulkInput(bulkInput);
    if (parsedEntries.length === 0) {
      setFeedback("Enter one or more PATH entries first.");
      return;
    }
    const nextDrafts = insertPathEntryDrafts(drafts, bulkInput, createId);
    const addedCount = nextDrafts.length - drafts.length;
    const skippedCount = parsedEntries.length - addedCount;
    if (addedCount === 0) {
      setFeedback("No entries added. Every pasted path is already present.");
      return;
    }
    onChange(nextDrafts);
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
      const nextDrafts = insertPathEntryDrafts(drafts, folder, createId);
      if (nextDrafts === drafts) {
        setFeedback("That folder is already in PATH. No changes made.");
        return;
      }
      onChange(nextDrafts);
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
    const nextDrafts = removeDuplicatePathEntryDrafts(drafts);
    const removedCount = drafts.length - nextDrafts.length;
    if (removedCount === 0) {
      setFeedback("No duplicate entries to remove.");
      return;
    }
    onChange(nextDrafts);
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
      onChange(reorderPathEntries(drafts, sourceIndex, targetIndex));
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
            <button className="secondary-button" disabled={disabled} onClick={() => onChange(appendPathEntryDraft(drafts, "", createId))} type="button"><Plus size={15} /> Add entry</button>
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
          {visibleEntries.map(({ draft, index, status }) => (
            <PathRow
              analysisError={Boolean(analysisError)}
              checking={checking}
              disabled={disabled}
              draft={draft}
              dragging={draggingIndex === index}
              entryCount={drafts.length}
              index={index}
              key={draft.id}
              onDragEnd={() => setDraggingIndex(null)}
              onDragStart={(event) => startDrag(event, index)}
              onDrop={(event) => dropEntry(event, index)}
              onMove={(direction) => move(index, direction)}
              onRemove={() => onChange(drafts.filter(({ id }) => id !== draft.id))}
              onUpdate={(value) => update(draft.id, value)}
              status={status}
            />
          ))}
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
