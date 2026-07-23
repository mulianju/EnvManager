import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import {
  ArrowRightLeft,
  Clipboard,
  Copy,
  Ellipsis,
  Eye,
  EyeOff,
  Pencil,
  Pin,
  PinOff,
  Terminal,
} from "lucide-react";
import {
  canTransferVariable,
  isSensitiveVariable,
} from "../lib/environment";
import { TablePagination, useTablePagination } from "./TablePagination";
import type {
  EffectiveEnvironmentVariable,
  EnvironmentScope,
  EnvironmentVariable,
  FavoriteKey,
  TransferMode,
  VariableCopyFormat,
} from "../types";

interface VariablesViewProps {
  variables: EnvironmentVariable[];
  query: string;
  canEdit: boolean;
  isElevated: boolean;
  busy: boolean;
  favorites: FavoriteKey[];
  activeMenuKey: string | null;
  paginationKey: string;
  onMenuChange: (key: string | null) => void;
  onOpen: (variable: EnvironmentVariable) => void;
  onCopy: (
    variable: Pick<EnvironmentVariable, "name" | "value">,
    format: VariableCopyFormat,
  ) => Promise<void>;
  onFavorite: (variable: EnvironmentVariable) => Promise<void>;
  onTransfer: (variable: EnvironmentVariable, mode: TransferMode) => Promise<void>;
}

export function VariablesView({
  variables,
  query,
  canEdit,
  isElevated,
  busy,
  favorites,
  activeMenuKey,
  paginationKey,
  onMenuChange,
  onOpen,
  onCopy,
  onFavorite,
  onTransfer,
}: VariablesViewProps) {
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  const menuRef = useRef<HTMLDivElement | null>(null);
  const pagination = useTablePagination(variables, paginationKey);
  useEffect(() => {
    if (!activeMenuKey) return;
    menuRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [activeMenuKey]);
  return (
    <section className="content-section">
      <div className="table-summary"><span>{variables.length} variables</span>{query && <span>Filtered by “{query}”</span>}</div>
      <div className="table-shell">
        <div className={activeMenuKey ? "variable-table menu-open" : "variable-table"}>
          <div className="variable-header"><span>Name</span><span>Value</span><span>Type</span><span /></div>
          {pagination.items.map((variable) => {
            const sensitive = isSensitiveVariable(variable.name);
            const key = variableKey(variable.scope, variable.name);
            const isRevealed = revealed.has(key);
            const favorite = isFavorite(favorites, variable);
            const menuOpen = activeMenuKey === key;
            const targetScope = variable.scope === "user" ? "system" : "user";
            return (
              <div
                className={canEdit ? "variable-row editable-row" : "variable-row"}
                key={key}
                onDoubleClick={(event) => {
                  if (!canEdit || (event.target as HTMLElement).closest("button")) return;
                  onOpen(variable);
                }}
              >
                <strong>{variable.name}</strong>
                <span className={sensitive && !isRevealed ? "value-preview masked" : "value-preview"}>{sensitive && !isRevealed ? "••••••••••••" : variable.value || "(empty)"}</span>
                <span className="type-label">{variable.valueType === "expandableString" ? "Expandable" : "String"}</span>
                <div className="row-actions">
                  {sensitive && <button className="icon-button small" aria-label={isRevealed ? "Hide value" : "Show value"} onClick={() => setRevealed((current) => toggleSetValue(current, key))} title={isRevealed ? "Hide value" : "Show value"} type="button">{isRevealed ? <EyeOff size={15} /> : <Eye size={15} />}</button>}
                  <div className="actions-menu-wrap">
                    <button
                      aria-expanded={menuOpen}
                      aria-haspopup="menu"
                      aria-label={`Actions for ${variable.name}`}
                      className="icon-button small"
                      onClick={() => onMenuChange(menuOpen ? null : key)}
                      title="Variable actions"
                      type="button"
                    >
                      <Ellipsis size={16} />
                    </button>
                    {menuOpen && (
                      <div
                        className="actions-menu"
                        onKeyDown={(event) => handleMenuKeyDown(event, () => onMenuChange(null))}
                        ref={menuRef}
                        role="menu"
                      >
                        <MenuButton disabled={!canEdit || busy} icon={<Pencil size={14} />} label="Edit" onClick={() => { onMenuChange(null); onOpen(variable); }} />
                        <MenuButton disabled={busy} icon={<Clipboard size={14} />} label="Copy name" onClick={() => void onCopy(variable, "name")} />
                        <MenuButton disabled={busy} icon={<Copy size={14} />} label="Copy value" onClick={() => void onCopy(variable, "value")} />
                        <MenuButton disabled={busy} icon={<Terminal size={14} />} label="Copy PowerShell" onClick={() => void onCopy(variable, "powershell")} />
                        <div className="menu-separator" />
                        <MenuButton disabled={busy} icon={favorite ? <PinOff size={14} /> : <Pin size={14} />} label={favorite ? "Unpin favorite" : "Pin favorite"} onClick={() => void onFavorite(variable)} />
                        <MenuButton
                          disabled={busy || !canTransferVariable(variable.scope, "copy", isElevated)}
                          icon={<Copy size={14} />}
                          label={`Copy to ${targetScope}`}
                          onClick={() => void onTransfer(variable, "copy")}
                        />
                        <MenuButton
                          disabled={busy || !canTransferVariable(variable.scope, "move", isElevated)}
                          icon={<ArrowRightLeft size={14} />}
                          label={`Move to ${targetScope}`}
                          onClick={() => void onTransfer(variable, "move")}
                        />
                      </div>
                    )}
                  </div>
                </div>
              </div>
            );
          })}
          {variables.length === 0 && <div className="table-empty">{query ? "No matching variables" : "No variables in this scope"}</div>}
        </div>
        <TablePagination
          {...pagination}
          onPageChange={(page) => {
            onMenuChange(null);
            pagination.setPage(page);
          }}
          onPageSizeChange={(pageSize) => {
            onMenuChange(null);
            pagination.setPageSize(pageSize);
          }}
        />
      </div>
    </section>
  );
}

export function EffectiveVariablesView({
  variables,
  query,
  busy,
  onCopy,
}: {
  variables: EffectiveEnvironmentVariable[];
  query: string;
  busy: boolean;
  onCopy: VariablesViewProps["onCopy"];
}) {
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  const pagination = useTablePagination(variables, `effective:${query}`);
  return (
    <section className="content-section">
      <div className="table-summary"><span>{variables.length} effective variables</span>{query && <span>Filtered by “{query}”</span>}</div>
      <div className="table-shell">
        <div className="variable-table effective-table">
          <div className="variable-header effective-header"><span>Name</span><span>Value</span><span>Source</span><span /></div>
          {pagination.items.map((variable) => {
            const key = `${variable.source}:${variable.name}`;
            const sensitive = isSensitiveVariable(variable.name);
            const isRevealed = revealed.has(key);
            return (
              <div className="variable-row effective-row" key={key}>
                <div className="effective-name">
                  <strong>{variable.name}</strong>
                  {variable.shadowed && (
                    <small title="The User value takes precedence over a System value with the same name">
                      User overrides System{variable.conflict ? " · values differ" : ""}
                    </small>
                  )}
                </div>
                <span className={sensitive && !isRevealed ? "value-preview masked" : "value-preview"}>{sensitive && !isRevealed ? "••••••••••••" : variable.value || "(empty)"}</span>
                <span className={`source-label ${variable.source}`}>{variable.source === "combined" ? "Combined PATH" : variable.source}</span>
                <div className="row-actions">
                  {sensitive && <button aria-label={isRevealed ? "Hide value" : "Show value"} className="icon-button small" onClick={() => setRevealed((current) => toggleSetValue(current, key))} title={isRevealed ? "Hide value" : "Show value"} type="button">{isRevealed ? <EyeOff size={15} /> : <Eye size={15} />}</button>}
                  <button aria-label={`Copy ${variable.name} name`} className="icon-button small" disabled={busy} onClick={() => void onCopy(variable, "name")} title="Copy name" type="button"><Clipboard size={15} /></button>
                  <button aria-label={`Copy ${variable.name} value`} className="icon-button small" disabled={busy} onClick={() => void onCopy(variable, "value")} title="Copy value" type="button"><Copy size={15} /></button>
                  <button aria-label={`Copy ${variable.name} PowerShell reference`} className="icon-button small" disabled={busy} onClick={() => void onCopy(variable, "powershell")} title="Copy PowerShell" type="button"><Terminal size={15} /></button>
                </div>
              </div>
            );
          })}
          {variables.length === 0 && <div className="table-empty">{query ? "No matching effective variables" : "No effective variables"}</div>}
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

function MenuButton({
  icon,
  label,
  disabled,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  disabled?: boolean;
  onClick: () => void;
}) {
  return <button disabled={disabled} onClick={onClick} role="menuitem" type="button">{icon}<span>{label}</span></button>;
}

function toggleSetValue(current: Set<string>, value: string): Set<string> {
  const next = new Set(current);
  if (next.has(value)) next.delete(value);
  else next.add(value);
  return next;
}

function variableKey(scope: EnvironmentScope, name: string): string {
  return `${scope}:${name}`;
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

function handleMenuKeyDown(
  event: ReactKeyboardEvent<HTMLDivElement>,
  close: () => void,
) {
  const items = Array.from(
    event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"),
  );
  if (items.length === 0) return;
  const currentIndex = items.indexOf(document.activeElement as HTMLButtonElement);
  let targetIndex: number | null = null;
  if (event.key === "ArrowDown") targetIndex = (currentIndex + 1) % items.length;
  if (event.key === "ArrowUp") {
    targetIndex = (currentIndex - 1 + items.length) % items.length;
  }
  if (event.key === "Home") targetIndex = 0;
  if (event.key === "End") targetIndex = items.length - 1;
  if (event.key === "Escape") {
    event.preventDefault();
    close();
    return;
  }
  if (targetIndex !== null) {
    event.preventDefault();
    items[targetIndex].focus();
  }
}
