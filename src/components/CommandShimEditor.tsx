import { useState } from "react";
import { FileSearch, LoaderCircle, Plus, Save, Trash2, X } from "lucide-react";
import { pickCommandShimArgument, pickCommandShimExecutable } from "../lib/api";
import { commandPreviewParts } from "../lib/command-shims";
import type { CommandShimInput } from "../types";

export function CommandShimEditor({
  input,
  busy,
  onClose,
  onDelete,
  onError,
  onSave,
}: {
  input: CommandShimInput;
  busy: boolean;
  onClose: () => void;
  onDelete: () => void;
  onError: (message: string) => void;
  onSave: (input: CommandShimInput) => void;
}) {
  const [draft, setDraft] = useState(input);
  const preview = commandPreviewParts(draft);

  const chooseExecutable = async () => {
    try {
      const path = await pickCommandShimExecutable();
      if (path) setDraft((current) => ({ ...current, executable: path }));
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    }
  };

  const chooseArgument = async (index: number) => {
    try {
      const path = await pickCommandShimArgument();
      if (!path) return;
      setDraft((current) => ({
        ...current,
        fixedArguments: current.fixedArguments.map((argument, argumentIndex) =>
          argumentIndex === index ? path : argument
        ),
      }));
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <div className="modal-backdrop" role="presentation">
      <section aria-labelledby="command-shim-editor-title" aria-modal="true" className="editor-modal command-shim-modal" role="dialog">
        <header className="modal-header">
          <div>
            <h2 id="command-shim-editor-title">{input.id ? `Edit ${input.commandName}` : "New Command Shim"}</h2>
            <span>User command</span>
          </div>
          <button aria-label="Close editor" className="icon-button" onClick={onClose} type="button"><X size={17} /></button>
        </header>
        <div className="modal-body command-shim-editor-body">
          <label>
            <span>Command name</span>
            <input autoFocus disabled={busy} placeholder="sharedev" value={draft.commandName} onChange={(event) => setDraft({ ...draft, commandName: event.target.value })} />
          </label>
          <label>
            <span>Executable</span>
            <div className="shim-path-input">
              <input disabled={busy} placeholder="C:\\path\\to\\runtime.exe" value={draft.executable} onChange={(event) => setDraft({ ...draft, executable: event.target.value })} />
              <button aria-label="Choose executable" className="secondary-button icon-only-button" disabled={busy} onClick={() => void chooseExecutable()} title="Choose executable" type="button"><FileSearch size={16} /></button>
            </div>
          </label>

          <section className="shim-argument-editor">
            <header>
              <div><strong>Fixed arguments</strong><span>{draft.fixedArguments.length}</span></div>
              <button className="secondary-button compact-button" disabled={busy} onClick={() => setDraft({ ...draft, fixedArguments: [...draft.fixedArguments, ""] })} type="button"><Plus size={15} /> Add argument</button>
            </header>
            <div className="shim-argument-list">
              {draft.fixedArguments.map((argument, index) => (
                <div className="shim-argument-row" key={index}>
                  <span>{index + 1}</span>
                  <input aria-label={`Fixed argument ${index + 1}`} disabled={busy} value={argument} onChange={(event) => setDraft({
                    ...draft,
                    fixedArguments: draft.fixedArguments.map((value, valueIndex) => valueIndex === index ? event.target.value : value),
                  })} />
                  <button aria-label={`Choose file for argument ${index + 1}`} className="icon-button small" disabled={busy} onClick={() => void chooseArgument(index)} title="Choose file" type="button"><FileSearch size={15} /></button>
                  <button aria-label={`Remove argument ${index + 1}`} className="icon-button small" disabled={busy} onClick={() => setDraft({ ...draft, fixedArguments: draft.fixedArguments.filter((_, valueIndex) => valueIndex !== index) })} title="Remove argument" type="button"><Trash2 size={15} /></button>
                </div>
              ))}
              {draft.fixedArguments.length === 0 && <div className="shim-argument-empty">No fixed arguments</div>}
            </div>
          </section>

          <section className="command-preview" aria-label="Execution preview">
            <strong>Execution preview</strong>
            <div>
              {preview.map((part, index) => <code className={part.kind} key={`${part.kind}:${index}`}>{part.value}</code>)}
            </div>
          </section>
        </div>
        <footer className="modal-footer">
          <div>{input.id && <button className="danger-button" disabled={busy} onClick={onDelete} type="button"><Trash2 size={16} /> Delete</button>}</div>
          <div className="button-row">
            <button className="secondary-button" onClick={onClose} type="button">Cancel</button>
            <button className="primary-button" disabled={busy || !draft.commandName.trim() || !draft.executable.trim()} onClick={() => onSave(draft)} type="button">
              {busy ? <LoaderCircle className="spin" size={16} /> : <Save size={16} />} Save
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}
