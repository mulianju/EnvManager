import { AlertTriangle, CheckCircle2, Pencil, Terminal, Wrench } from "lucide-react";
import {
  commandShimAccessNeedsRepair,
  commandShimStatusLabel,
} from "../lib/command-shims";
import type { CommandShim, CommandShimSnapshot } from "../types";
import { TablePagination, useTablePagination } from "./TablePagination";

export function CommandShimsView({
  snapshot,
  items,
  query,
  busy,
  onEdit,
  onRepair,
}: {
  snapshot: CommandShimSnapshot;
  items: CommandShim[];
  query: string;
  busy: boolean;
  onEdit: (item: CommandShim) => void;
  onRepair: () => void;
}) {
  const pagination = useTablePagination(items, `command-shims:${query}`);
  const needsRepair = commandShimAccessNeedsRepair(snapshot);
  const accessMessage = !snapshot.pathReady
    ? "Missing from User PATH"
    : needsRepair
      ? "One or more managed wrappers are missing"
      : "Available in User PATH";
  return (
    <section className="content-section command-shims-section">
      <div className={needsRepair ? "shim-directory warning" : "shim-directory ready"}>
        {needsRepair ? <AlertTriangle size={17} /> : <CheckCircle2 size={17} />}
        <div>
          <code>{snapshot.managedDirectory}</code>
          <span>{accessMessage}</span>
        </div>
        {needsRepair && (
          <button className="secondary-button compact-button" disabled={busy} onClick={onRepair} type="button">
            <Wrench size={14} /> Repair shell access
          </button>
        )}
      </div>
      <div className="table-shell">
        <div className="command-shim-table">
          <div className="command-shim-header">
            <span>Command</span><span>Executable</span><span>Fixed arguments</span><span>Status</span><span />
          </div>
          {pagination.items.map((item) => (
            <div className="command-shim-row" key={item.id}>
              <div className="shim-command"><Terminal size={15} /><strong>{item.commandName}</strong></div>
              <code title={item.executable}>{item.executable}</code>
              <div className="shim-arguments" title={item.fixedArguments.join("\n")}>
                {item.fixedArguments.length
                  ? item.fixedArguments.map((argument, index) => <code key={`${index}:${argument}`}>{argument || "(empty)"}</code>)
                  : <span>None</span>}
              </div>
              <span className={`shim-status ${item.status}`} title={item.statusMessage ?? undefined}>
                {commandShimStatusLabel(item.status)}
              </span>
              <button
                aria-label={`Edit ${item.commandName}`}
                className="icon-button small"
                disabled={busy}
                onClick={() => onEdit(item)}
                title={`Edit ${item.commandName}`}
                type="button"
              >
                <Pencil size={15} />
              </button>
            </div>
          ))}
          {items.length === 0 && (
            <div className="table-empty">
              {query ? "No Command Shims match this search" : "No Command Shims configured"}
            </div>
          )}
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
