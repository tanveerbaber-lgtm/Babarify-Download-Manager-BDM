import { useEffect } from "react";
import { X } from "lucide-react";

export interface ToastMessage {
  id: number;
  text: string;
}

/** Short-lived notices stacked in the bottom-right corner. */
export function Toasts({ toasts, onDismiss }: { toasts: ToastMessage[]; onDismiss: (id: number) => void }) {
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((t) => (
        <Toast key={t.id} toast={t} onDismiss={onDismiss} />
      ))}
    </div>
  );
}

function Toast({ toast, onDismiss }: { toast: ToastMessage; onDismiss: (id: number) => void }) {
  useEffect(() => {
    const timer = setTimeout(() => onDismiss(toast.id), 5000);
    return () => clearTimeout(timer);
  }, [toast.id, onDismiss]);
  return (
    <div className="toast">
      <span>{toast.text}</span>
      <button type="button" className="icon-button" aria-label="Dismiss" onClick={() => onDismiss(toast.id)}>
        <X size={15} />
      </button>
    </div>
  );
}
