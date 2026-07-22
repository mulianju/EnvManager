import type { DragEvent as ReactDragEvent } from "react";
import {
  ArrowDown,
  ArrowUp,
  CircleHelp,
  FolderCheck,
  FolderX,
  GripVertical,
  LoaderCircle,
  Trash2,
} from "lucide-react";
import type { PathEntryDraft, PathEntryStatus } from "../types";

interface PathRowProps {
  analysisError: boolean;
  checking: boolean;
  disabled: boolean;
  draft: PathEntryDraft;
  dragging: boolean;
  entryCount: number;
  index: number;
  onDragEnd: () => void;
  onDragStart: (event: ReactDragEvent<HTMLElement>) => void;
  onDrop: (event: ReactDragEvent<HTMLDivElement>) => void;
  onMove: (direction: -1 | 1) => void;
  onRemove: () => void;
  onUpdate: (value: string) => void;
  status: PathEntryStatus | undefined;
}

export function PathRow({
  analysisError,
  checking,
  disabled,
  draft,
  dragging,
  entryCount,
  index,
  onDragEnd,
  onDragStart,
  onDrop,
  onMove,
  onRemove,
  onUpdate,
  status,
}: PathRowProps) {
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
      className={`path-row${status?.duplicate ? " duplicate" : ""}${status && !status.exists ? " missing" : ""}${dragging ? " dragging" : ""}`}
      onDragEnd={onDragEnd}
      onDragOver={(event) => { if (!disabled) event.preventDefault(); }}
      onDrop={(event) => { if (!disabled) onDrop(event); }}
    >
      <span className="path-drag-handle" draggable={!disabled} onDragStart={onDragStart} title="Drag to reorder"><GripVertical size={15} /></span>
      <span className="path-index">{index + 1}</span>
      <div className="path-input"><input aria-label={`PATH entry ${index + 1}`} disabled={disabled} value={draft.value} onChange={(event) => onUpdate(event.target.value)} /><small title={status?.expandedValue}>{statusLabel}</small></div>
      <span className={`path-status ${statusClass}`}>{statusIcon}</span>
      <div className="path-actions"><button aria-label="Move up" className="icon-button small" disabled={disabled || index === 0} onClick={() => onMove(-1)} type="button"><ArrowUp size={14} /></button><button aria-label="Move down" className="icon-button small" disabled={disabled || index === entryCount - 1} onClick={() => onMove(1)} type="button"><ArrowDown size={14} /></button><button aria-label="Remove entry" className="icon-button small danger-icon" disabled={disabled} onClick={onRemove} type="button"><Trash2 size={14} /></button></div>
    </div>
  );
}
