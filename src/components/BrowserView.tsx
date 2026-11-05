import { useEffect, useState } from "react";
import { CircleCheck, CircleAlert, ExternalLink, Globe, Link2Off } from "lucide-react";
import { api } from "../lib/api";
import type { AppError, BrowserStatus, Settings, SettingsPatch } from "../lib/types";

interface BrowserViewProps {
  settings: Settings;
  onUpdate: (patch: SettingsPatch) => Promise<Settings>;
}

/** Native-messaging registration for Chrome and Edge, and how to add the extension. */
export function BrowserView({ settings, onUpdate }: BrowserViewProps) {
  const [status, setStatus] = useState<BrowserStatus[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getBrowserIntegration().then(setStatus).catch((e: AppError) => setError(e.message));
  }, []);

  async function run(action: () => Promise<BrowserStatus[]>) {
    setBusy(true);
    setError(null);
    try {
      setStatus(await action());
    } catch (e) {
      setError((e as AppError).message);
    } finally {
      setBusy(false);
    }
  }

  const allRegistered = status?.every((s) => s.registered && s.hostFound) ?? false;

  return (
    <div className="settings">
      <section className="card">
        <h2>Connection</h2>
        <p className="card-intro">
          The BDM extension for Chrome and Edge hands downloads and video pages to BDM through a small helper that only the BDM
          extension may start. Registration is per Windows user and needs no administrator rights.
        </p>
        {!status && <p className="hint">Checking…</p>}
        {status?.map((s) => (
          <div key={s.browser} className="setting-row">
            <div className="row-with-icon">
              <Globe size={18} aria-hidden />
              <div className="setting-title">{s.name}</div>
            </div>
            {s.registered && s.hostFound ? (
              <span className="status status-completed">
                <span className="status-dot" aria-hidden /> Ready
              </span>
            ) : (
              <span className="status status-pending">
                <span className="status-dot" aria-hidden /> Not set up
              </span>
            )}
          </div>
        ))}
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <div className="card-actions">
          {allRegistered && (
            <button type="button" className="button" disabled={busy} onClick={() => run(api.unregisterBrowserIntegration)}>
              <Link2Off size={15} aria-hidden />
              Disconnect browsers
            </button>
          )}
          <button type="button" className="button primary" disabled={busy} onClick={() => run(api.registerBrowserIntegration)}>
            {allRegistered ? <CircleCheck size={15} aria-hidden /> : <CircleAlert size={15} aria-hidden />}
            {allRegistered ? "Repair connection" : "Connect Chrome and Edge"}
          </button>
        </div>
      </section>

      <section className="card">
        <h2>Add the extension</h2>
        <div className="setting-row">
          <div>
            <div className="setting-title">Google Chrome</div>
            <div className="setting-desc">
              Choose <strong>Add to Chrome</strong> on the store page. If it opens in another browser, open it in Chrome instead.
            </div>
          </div>
          <button
            type="button"
            className="button primary"
            onClick={() => api.openChromeStore().catch((e: AppError) => setError(e.message))}
          >
            <ExternalLink size={15} aria-hidden />
            Open Chrome Web Store
          </button>
        </div>
        <div className="setting-row">
          <div>
            <div className="setting-title">Microsoft Edge</div>
            <div className="setting-desc">Install it manually until the Edge Add-ons listing is ready.</div>
          </div>
        </div>
        <ol className="steps">
          <li>Unzip the browser extension from the BDM download page.</li>
          <li>
            Open <code>edge://extensions</code> and turn on <strong>Developer mode</strong>.
          </li>
          <li>
            Choose <strong>Load unpacked</strong> and select the unzipped folder.
          </li>
        </ol>
        <div className="setting-row">
          <div>
            <div className="setting-title">Then, in either browser</div>
          </div>
        </div>
        <ol className="steps">
          <li>Pin the BDM button to the toolbar. Its popup shows whether it can reach BDM.</li>
          <li>
            If the popup says BDM isn't connected, <strong>restart the browser</strong> once.
          </li>
        </ol>
      </section>

      <section className="card">
        <h2>Behaviour</h2>
        <div className="setting-row">
          <label htmlFor="show-on-browser">
            <div className="setting-title">Bring BDM to the front</div>
            <div className="setting-desc">Show BDM when the browser sends it a download.</div>
          </label>
          <input
            id="show-on-browser"
            type="checkbox"
            className="switch"
            checked={settings.showOnBrowserAdd}
            onChange={(e) => void onUpdate({ showOnBrowserAdd: e.target.checked })}
          />
        </div>
        <p className="hint">
          Choose which downloads BDM takes over in the extension's options. For downloads that need you to be signed in,
          allow the site under "Sign-in downloads" there, and the extension shares that site's sign-in with BDM.
        </p>
      </section>
    </div>
  );
}
