import { Modal } from "./Modal";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({ open, title, message, confirmLabel, onConfirm, onCancel }: ConfirmDialogProps) {
  return (
    <Modal open={open} title={title} onClose={onCancel} className="confirm-dialog">
      <div className="modal-body">
        <h2>{title}</h2>
        <p>{message}</p>
      </div>
      <footer className="modal-footer">
        <button type="button" className="button" onClick={onCancel} autoFocus>
          Cancel
        </button>
        <button type="button" className="button danger" onClick={onConfirm}>
          {confirmLabel}
        </button>
      </footer>
    </Modal>
  );
}
