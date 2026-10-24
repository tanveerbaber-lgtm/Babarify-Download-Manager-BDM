import { useState, type FormEvent } from "react";
import { Trash2, X } from "lucide-react";
import { Modal } from "./Modal";
import { api } from "../lib/api";
import type { AppError, Download, Queue, QueueSchedule } from "../lib/types";

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

interface QueueDialogProps {
  /** `null` creates a new queue. */
  queue: Queue | null;
  onClose: () => void;
  /** After a delete, the downloads that moved to Main. */
  onSaved: (moved?: Download[]) => void;
}

export function QueueDialog({ queue, onClose, onSaved }: QueueDialogProps) {
  return (
    <Modal open title={queue ? "Edit queue" : "New queue"} onClose={onClose} className="queue-dialog">
      <QueueForm queue={queue} onClose={onClose} onSaved={onSaved} />
    </Modal>
  );
}

function QueueForm({ queue, onClose, onSaved }: QueueDialogProps) {
  const initial: QueueSchedule = queue?.schedule ?? { enabled: true, start: "01:00", stop: "07:00", days: [0, 1, 2, 3, 4, 5, 6] };
  const [name, setName] = useState(queue?.name ?? "");
  const [enabled, setEnabled] = useState(initial.enabled);
  const [start, setStart] = useState(initial.start);
  const [hasStop, setHasStop] = useState(initial.stop !== null);
  const [stop, setStop] = useState(initial.stop ?? "07:00");
  const [days, setDays] = useState<number[]>(initial.days);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const isMain = queue?.id === "main";

  function toggleDay(d: number) {
    setDays((prev) => (prev.includes(d) ? prev.filter((x) => x !== d) : [...prev, d].sort()));
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.saveQueue(name, { enabled, start, stop: hasStop ? stop : null, days }, queue?.id);
      onSaved();
      onClose();
    } catch (err) {
      setError((err as AppError).message);
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!queue) return;
    setBusy(true);
    try {
      onSaved(await api.deleteQueue(queue.id));
      onClose();
    } catch (err) {
      setError((err as AppError).message);
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} noValidate>
      <header className="modal-header">
        <h2>{queue ? "Edit Queue" : "New Queue"}</h2>
        <button type="button" className="icon-button" aria-label="Close" onClick={onClose}>
          <X size={18} />
        </button>
      </header>
      <div className="modal-body">
        <label className="field">
          <span className="field-label">Name</span>
          <input type="text" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Overnight" maxLength={40} autoFocus />
        </label>

        <label className="check">
          <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
          Run on a schedule
        </label>

        {enabled && (
          <fieldset className="schedule">
            <div className="schedule-times">
              <label className="field">
                <span className="field-label">Start at</span>
                <input type="time" className="time-input" value={start} onChange={(e) => setStart(e.target.value)} required />
              </label>
              <label className="field">
                <span className="field-label">
                  <input type="checkbox" checked={hasStop} onChange={(e) => setHasStop(e.target.checked)} /> Stop at
                </span>
                <input type="time" className="time-input" value={stop} disabled={!hasStop} onChange={(e) => setStop(e.target.value)} />
              </label>
            </div>
            <div className="field">
              <span className="field-label">On</span>
              <div className="day-chips" role="group" aria-label="Days">
                {DAYS.map((label, d) => (
                  <button key={label} type="button" className="chip" aria-pressed={days.includes(d)} onClick={() => toggleDay(d)}>
                    {label}
                  </button>
                ))}
              </div>
            </div>
            <p className="hint">
              {hasStop
                ? "Downloads in this queue start at the start time and pause at the stop time, keeping their progress. A stop time earlier than the start runs overnight."
                : "Downloads in this queue start at the start time and run until the queue is empty."}
            </p>
          </fieldset>
        )}
        {!enabled && <p className="hint">Start and stop this queue yourself from the sidebar.</p>}

        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <footer className="modal-footer">
        {queue && !isMain && (
          <button type="button" className="button danger footer-left" onClick={remove} disabled={busy} title="Its downloads move to Main">
            <Trash2 size={15} aria-hidden />
            Delete
          </button>
        )}
        <button type="button" className="button" onClick={onClose}>
          Cancel
        </button>
        <button type="submit" className="button primary" disabled={busy || !name.trim()}>
          {queue ? "Save" : "Create Queue"}
        </button>
      </footer>
    </form>
  );
}
