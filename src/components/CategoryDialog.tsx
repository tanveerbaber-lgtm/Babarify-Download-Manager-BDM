import { useState, type FormEvent } from "react";
import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Trash2, X } from "lucide-react";
import { Modal } from "./Modal";
import { api } from "../lib/api";
import type { AppError, Category } from "../lib/types";

interface CategoryDialogProps {
  /** `null` creates a new category. */
  category: Category | null;
  defaultDir: string;
  onClose: () => void;
  onSaved: () => void;
}

export function CategoryDialog(props: CategoryDialogProps) {
  return (
    <Modal open title={props.category ? "Edit category" : "New category"} onClose={props.onClose}>
      <CategoryForm {...props} />
    </Modal>
  );
}

function CategoryForm({ category, defaultDir, onClose, onSaved }: CategoryDialogProps) {
  const [name, setName] = useState(category?.name ?? "");
  const [extensions, setExtensions] = useState(category?.extensions.join(", ") ?? "");
  const [folder, setFolder] = useState(category?.folder ?? "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function browse() {
    const picked = await openFolderPicker({ directory: true, multiple: false, defaultPath: folder || defaultDir, title: "Choose the folder for this category" });
    if (typeof picked === "string") setFolder(picked);
  }

  async function run(action: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      onSaved();
      onClose();
    } catch (err) {
      setError((err as AppError).message);
      setBusy(false);
    }
  }

  function submit(e: FormEvent) {
    e.preventDefault();
    void run(() => api.saveCategory(name, [extensions], folder.trim() || null, category?.id));
  }

  const automatic = `${defaultDir.replace(/[\\/]+$/, "")}\\${name.trim() || "Category"}`;

  return (
    <form onSubmit={submit} noValidate>
      <header className="modal-header">
        <h2>{category ? "Edit Category" : "New Category"}</h2>
        <button type="button" className="icon-button" aria-label="Close" onClick={onClose}>
          <X size={18} />
        </button>
      </header>
      <div className="modal-body">
        <label className="field">
          <span className="field-label">Name</span>
          <input type="text" value={name} onChange={(e) => setName(e.target.value)} maxLength={40} placeholder="e.g. E-books" autoFocus />
        </label>
        <label className="field">
          <span className="field-label">File types</span>
          <input type="text" value={extensions} onChange={(e) => setExtensions(e.target.value)} placeholder="epub, mobi, azw3" spellCheck={false} />
          <span className="hint">Extensions separated by commas or spaces.</span>
        </label>
        <div className="field">
          <span className="field-label" id="cat-folder">Folder</span>
          <div className="input-row">
            <input type="text" aria-labelledby="cat-folder" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder={automatic} spellCheck={false} />
            <button type="button" className="button" onClick={browse}>
              <FolderOpen size={16} aria-hidden />
              Browse
            </button>
          </div>
          <span className="hint">Leave empty to use a subfolder of your download folder.</span>
        </div>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <footer className="modal-footer">
        {category && (
          <button type="button" className="button danger footer-left" disabled={busy} onClick={() => run(() => api.deleteCategory(category.id))}>
            <Trash2 size={15} aria-hidden />
            Delete
          </button>
        )}
        <button type="button" className="button" onClick={onClose}>
          Cancel
        </button>
        <button type="submit" className="button primary" disabled={busy || !name.trim() || !extensions.trim()}>
          Save
        </button>
      </footer>
    </form>
  );
}
