import { useState, type FormEvent } from "react";
import { Modal } from "./Modal";

interface SpeedLimitDialogProps {
  open: boolean;
  count: number;
  /** Current limit in KB/s, if all selected downloads share one. */
  current: number | null;
  onClose: () => void;
  /** `null` removes the limit. */
  onApply: (kbps: number | null) => void;
}

const PRESETS = [256, 512, 1024, 2048, 5120];

export function SpeedLimitDialog({ open, count, current, onClose, onApply }: SpeedLimitDialogProps) {
  return (
    <Modal open={open} title="Speed limit" onClose={onClose} className="confirm-dialog">
      <SpeedLimitForm count={count} current={current} onClose={onClose} onApply={onApply} />
    </Modal>
  );
}

function SpeedLimitForm({ count, current, onClose, onApply }: Omit<SpeedLimitDialogProps, "open">) {
  const [value, setValue] = useState(current ? String(current) : "1024");
  const n = Number(value);
  const valid = Number.isInteger(n) && n >= 1 && n <= 10_000_000;

  function submit(e: FormEvent) {
    e.preventDefault();
    if (!valid) return;
    onApply(n);
    onClose();
  }

  return (
    <form onSubmit={submit}>
      <div className="modal-body">
        <h2>Speed Limit</h2>
        <p>
          Limit {count === 1 ? "this download" : `these ${count} downloads`} to a maximum speed. The global limit in Settings
          still applies on top.
        </p>
        <label className="field">
          <span className="field-label">Maximum speed (KB/s)</span>
          <input type="text" inputMode="numeric" value={value} onChange={(e) => setValue(e.target.value.replace(/\D/g, ""))} autoFocus />
        </label>
        <div className="day-chips">
          {PRESETS.map((p) => (
            <button key={p} type="button" className="chip" aria-pressed={n === p} onClick={() => setValue(String(p))}>
              {p >= 1024 ? `${p / 1024} MB/s` : `${p} KB/s`}
            </button>
          ))}
        </div>
      </div>
      <footer className="modal-footer">
        <button
          type="button"
          className="button footer-left"
          onClick={() => {
            onApply(null);
            onClose();
          }}
        >
          No Limit
        </button>
        <button type="button" className="button" onClick={onClose}>
          Cancel
        </button>
        <button type="submit" className="button primary" disabled={!valid}>
          Apply
        </button>
      </footer>
    </form>
  );
}
