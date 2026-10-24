// Talking to BDM through the native-messaging host.

export const HOST = "com.babarify.bdm";

/**
 * Sends one message to BDM. Never throws: failures come back as
 * { ok: false, error, notInstalled? } so callers can fall back.
 */
export async function sendToBdm(message) {
  try {
    const reply = await chrome.runtime.sendNativeMessage(HOST, message);
    if (!reply || typeof reply !== "object") return { ok: false, error: "No reply from BDM." };
    return reply;
  } catch (err) {
    const text = String(err?.message ?? err);
    // Chrome/Edge wording when the host isn't registered for this user.
    const notInstalled = /not found|not registered|forbidden/i.test(text);
    return {
      ok: false,
      notInstalled,
      error: notInstalled
        ? "BDM isn't connected to this browser. Open BDM → Browser integration → Connect, then restart the browser. (Browsers running as administrator ignore per-user helpers.)"
        : text,
      // The browser's own wording, shown as a detail for troubleshooting.
      detail: text,
    };
  }
}

/** Connection state for the popup: "running", "stopped" (BDM starts on demand) or "missing". */
export async function connectionState() {
  const reply = await sendToBdm({ type: "ping" });
  if (reply.ok) return { state: reply.app === "running" ? "running" : "stopped", version: reply.version ?? null };
  return { state: "missing", error: reply.error, detail: reply.detail };
}
