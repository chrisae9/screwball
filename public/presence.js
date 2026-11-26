// Report only an existing push subscription. Each document has its own lease,
// including multiple windows of the same installed app.
if ("serviceWorker" in navigator && window.crypto?.randomUUID) {
  const windowId = crypto.randomUUID();
  let sequence = 0;
  let endpoint;
  let pageHidden = false;
  let lastVisible = false;
  let discovering = false;
  let nextLookup = 0;
  let nextRepair = 0;

  function visible() {
    return !pageHidden && document.visibilityState === "visible";
  }

  function report() {
    const active = visible();
    if (!endpoint || (!active && !lastVisible)) return;
    lastVisible = active;
    const current = ++sequence;
    const url = "/api/push/presence";
    const body = JSON.stringify({ endpoint, window_id: windowId, sequence: current, visible: active });
    if (!active && navigator.sendBeacon?.(url, new Blob([body], {type:"application/json"}))) return;
    fetch(url, {
      method: "POST", headers: {"Content-Type":"application/json"}, body,
      cache: "no-store", keepalive: !active, signal: AbortSignal.timeout(5000),
    }).then(response => {
      if (current !== sequence || ![401, 403, 404].includes(response.status)) return;
      const missing = endpoint;
      endpoint = null;
      lastVisible = false;
      nextLookup = Date.now() + 12000;
      if (response.status === 404 && active) void discover(missing);
    }).catch(() => {}); // The next visible heartbeat retries. Stale leases expire on the server.
  }

  async function discover(missing) {
    if (endpoint) { report(); return; }
    if (!visible() || discovering || !navigator.serviceWorker.getRegistration) return;
    if (!missing && Date.now() < nextLookup) return;
    if (!("Notification" in window) || Notification.permission !== "granted") return;
    discovering = true;
    nextLookup = Date.now() + 12000;
    try {
      const worker = await navigator.serviceWorker.getRegistration();
      const sub = await worker?.pushManager?.getSubscription();
      if (!sub?.endpoint || !visible() || endpoint) return;
      if (sub.endpoint === missing) {
        // A 404 means the signed-in account has no record of this endpoint.
        // Re-read the browser subscription before restoring it. Never prompt here.
        if (Date.now() < nextRepair) return;
        nextRepair = Date.now() + 30000;
        const saved = await fetch("/api/push/subscribe", {
          method: "POST", headers: {"Content-Type":"application/json"},
          body: JSON.stringify(sub.toJSON()), signal: AbortSignal.timeout(5000),
        });
        if (!saved.ok) return;
      }
      endpoint = sub.endpoint;
      report();
    } catch { /* Registration can become available later. */ }
    finally { discovering = false; }
  }

  window.addEventListener("scrabble-push-subscription", event => {
    endpoint = event.detail;
    report();
  });
  function resume() {
    nextLookup = 0;
    void discover();
  }
  document.addEventListener("visibilitychange", () => visible() ? resume() : report());
  window.addEventListener("focus", resume);
  window.addEventListener("online", resume);
  window.addEventListener("pageshow", () => { pageHidden = false; resume(); });
  window.addEventListener("pagehide", () => { pageHidden = true; report(); });
  setInterval(() => { if (visible()) void discover(); }, 4000);
  void discover();
}
