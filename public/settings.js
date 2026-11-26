function localizeAwayTime() {
  const time = document.getElementById("away-until");
  if (time) time.textContent = new Intl.DateTimeFormat(undefined, {
    weekday: "short", hour: "numeric", minute: "2-digit",
  }).format(new Date(time.dateTime));
}
localizeAwayTime();

let updatingAway = false;
window.addEventListener("scrabble-away", async event => {
  const panel = document.querySelector(".away-settings");
  if (!panel || updatingAway || event.detail === panel.dataset.awayState) return;
  // The server controls expiry. Update only this card, preserving other pending form edits.
  updatingAway = true;
  try {
    const response = await fetch("/settings", {cache: "no-store", signal: AbortSignal.timeout(8000)});
    if (!response.ok || response.redirected) return;
    const html = new DOMParser().parseFromString(await response.text(), "text/html");
    const next = html.querySelector(".away-settings");
    if (next) {
      panel.replaceWith(next);
      localizeAwayTime();
      void refreshStatus();
    }
  } catch { /* The next activity check retries while the two states differ. */ }
  finally { updatingAway = false; }
});

const statusEl = document.getElementById("notif-status");
const buttonEl = document.getElementById("notif-enable");

const browserSupport = () => ({
  notifications: "Notification" in window,
  serviceWorker: "serviceWorker" in navigator,
  pushManager: "PushManager" in window,
});

const setStatus = (value) => {
  if (statusEl) statusEl.textContent = value;
};

function urlBase64ToUint8Array(value) {
  const padding = "=".repeat((4 - (value.length % 4)) % 4);
  const base64 = (value + padding).replace(/-/g, "+").replace(/_/g, "/");
  const raw = window.atob(base64);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) {
    bytes[i] = raw.charCodeAt(i);
  }
  return bytes;
}

async function jsonFetch(url, options = {}) {
  const response = await fetch(url, options);
  const text = await response.text();
  let body = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = text;
  }
  if (!response.ok) {
    const message = body && body.error ? body.error : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return body;
}

async function enableNotifications() {
  const support = browserSupport();
  if (!support.notifications || !support.serviceWorker || !support.pushManager) {
    throw new Error("this browser does not support all notification APIs");
  }
  let permission = Notification.permission;
  if (permission === "default") {
    permission = await Notification.requestPermission();
  }
  if (permission !== "granted") {
    throw new Error(`notification permission is ${permission}`);
  }

  const { public_key: publicKey } = await jsonFetch("/api/push/vapid-public-key");
  if (!publicKey) {
    throw new Error("server does not have a VAPID public key");
  }

  await navigator.serviceWorker.register("/sw.js");
  const registration = await navigator.serviceWorker.ready;
  let subscription = await registration.pushManager.getSubscription();
  if (!subscription) {
    subscription = await registration.pushManager.subscribe({
      userVisibleOnly: true,
      applicationServerKey: urlBase64ToUint8Array(publicKey),
    });
  }
  await jsonFetch("/api/push/subscribe", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(subscription.toJSON()),
  });
  window.dispatchEvent(new CustomEvent("scrabble-push-subscription", {detail: subscription.endpoint}));
}

const homePanel = document.getElementById("phone-alerts");
let updating = false;

async function refreshStatus(prompt = false) {
  if (updating || !statusEl || !buttonEl || document.hidden) return;
  if (homePanel?.dataset.away === "true") {
    homePanel.hidden = false;
    setStatus("Away mode is on. You can resume alerts in Settings.");
    buttonEl.hidden = true;
    return;
  }
  const ios = /iPad|iPhone|iPod/.test(navigator.userAgent)
    || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
  if (homePanel) homePanel.hidden = false;
  if (ios && !(navigator.standalone || matchMedia("(display-mode: standalone)").matches)) {
    setStatus("For iPhone or iPad alerts, add Scrabble to your Home Screen, then open it there and enable notifications.");
    buttonEl.hidden = true;
    return;
  }
  const support = browserSupport();
  if (!support.notifications || !support.serviceWorker || !support.pushManager) {
    setStatus("Notifications aren't supported in this browser.");
    buttonEl.hidden = true;
    if (homePanel) homePanel.hidden = true;
    return;
  }
  if (Notification.permission === "denied") {
    setStatus("Notifications are blocked in your browser settings.");
    buttonEl.hidden = true;
    return;
  }
  buttonEl.textContent = "Enable notifications";
  if (!prompt && Notification.permission !== "granted") {
    setStatus(homePanel ? "Get an alert when someone invites you or it’s your turn." : "Notifications are off.");
    buttonEl.hidden = false;
    return;
  }
  updating = true;
  buttonEl.disabled = true;
  try {
    // Re-save the current device even when its browser already has a subscription.
    await enableNotifications();
    setStatus(document.querySelector('.away-settings:not([data-away-state="off"])') ? "This device is registered for notifications." : "Notifications are on.");
    buttonEl.hidden = true;
    if (homePanel) homePanel.hidden = true;
  } catch {
    const denied = Notification.permission === "denied";
    setStatus(denied ? "Notifications are blocked in your browser settings." : "Phone alerts aren't connected yet. Tap to retry.");
    buttonEl.hidden = denied;
  } finally {
    buttonEl.disabled = false;
    updating = false;
  }
}
homePanel?.addEventListener("awaychange", () => refreshStatus());
buttonEl?.addEventListener("click", () => refreshStatus(true));
document.addEventListener("visibilitychange", () => refreshStatus());
window.addEventListener("online", () => refreshStatus());
window.addEventListener("pageshow", event => { if (event.persisted) refreshStatus(); });
refreshStatus();


const avatarPicker = document.querySelector(".avatar-picker");
avatarPicker?.addEventListener("change", event => {
  if (event.target.name === "avatar_id") {
    avatarPicker.querySelector(".avatar-selected").value = event.target.getAttribute("aria-label");
  }
});
