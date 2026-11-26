function setupInstallGuide(guide) {
  if (!guide) return;
  const standalone = matchMedia("(display-mode: standalone)");
  const ua = navigator.userAgent;
  const ios = /CriOS\//.test(ua);
  const android = /Android/.test(ua) && /Chrome\//.test(ua)
    && !/EdgA|OPR\/|SamsungBrowser|Firefox|; wv\)/.test(ua);
  if (!(ios || android) || standalone.matches || navigator.standalone) return;
  const steps = ios
    ? ["Tap Share beside the address bar in Chrome.", "Choose Add to Home Screen. Scroll down in the share sheet if needed.", "Keep the name Scrabble and tap Add."]
    : ["Tap the three-dot menu in Chrome.", "Choose Install and create shortcut or Add to Home screen.", "Tap Install and confirm."];
  for (const step of steps) {
    const li = document.createElement("li");
    li.textContent = step;
    guide.querySelector("ol").append(li);
  }
  guide.hidden = false;
  window.addEventListener("appinstalled", () => { guide.hidden = true; });
  standalone.addEventListener("change", () => { guide.hidden = standalone.matches; });
}

const inviteForm = document.querySelector("[data-invite-form]");
let directoryRequest;
let requestedAt = 0;

function getPlayers() {
  if (!directoryRequest || Date.now() - requestedAt > 30_000) {
    requestedAt = Date.now();
    const game = inviteForm?.dataset.inviteForm;
    const url = "/api/players" + (game ? "?game=" + encodeURIComponent(game) : "");
    directoryRequest = fetch(url, {cache: "no-store"}).then(response => {
      if (!response.ok) throw new Error("Directory unavailable");
      return response.json();
    }).catch(error => {
      directoryRequest = null;
      throw error;
    });
  }
  return directoryRequest;
}

function playerPicker(label, choose, otherSeats) {
  const panel = document.createElement("details");
  panel.className = "player-picker";
  panel.innerHTML = `<summary><span class="picker-title">Choose a player</span><span class="picker-action"></span></summary>
    <div class="picker-content"><input type="search" placeholder="Filter by name" autocomplete="off">
    <ul class="picker-roster" aria-label="Available players"></ul><div class="picker-status" role="status"></div></div>`;
  const summary = panel.querySelector("summary");
  const title = panel.querySelector(".picker-title");
  const action = panel.querySelector(".picker-action");
  const input = panel.querySelector("input");
  const list = panel.querySelector("ul");
  const status = panel.querySelector("[role=status]");
  input.setAttribute("aria-label", label);
  let selected = null;
  let players = [];
  let counts = new Map();

  function select(player) {
    selected = player;
    title.textContent = player ? player.name : "Choose a player";
    action.textContent = player ? "Change" : "";
    choose(player);
  }

  function render() {
    const query = input.value.trim().toLowerCase();
    const excluded = otherSeats();
    const rows = [];
    for (const player of players) {
      if (excluded.has(player.id) || !player.name.toLowerCase().includes(query)) continue;
      const row = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.className = "picker-choice";
      const initial = document.createElement("span");
      initial.className = "picker-initial";
      initial.setAttribute("aria-hidden", "true");
      initial.textContent = [...player.name][0]?.toUpperCase() || "·";
      const name = document.createElement("span");
      name.textContent = counts.get(player.name) > 1 ? player.name + " · " + player.id.slice(0, 6) : player.name;
      button.append(initial, name);
      if (player.id === selected?.id) button.setAttribute("aria-current", "true");
      button.addEventListener("click", () => {
        select(player);
        panel.open = false;
        summary.focus();
      });
      row.append(button);
      rows.push(row);
    }
    list.replaceChildren(...rows);
    list.scrollTop = 0;
    status.textContent = rows.length ? "" : query ? "No matching players." : "No other players available.";
  }

  async function browse() {
    // Keep the on-screen keyboard closed until the player taps the filter.
    for (const other of document.querySelectorAll(".player-picker[open]")) {
      if (other !== panel) other.open = false;
    }
    input.value = "";
    input.disabled = true;
    list.replaceChildren();
    status.textContent = "Loading players…";
    try {
      players = await getPlayers();
      counts = new Map();
      for (const player of players) counts.set(player.name, (counts.get(player.name) || 0) + 1);
      render();
      input.disabled = false;
    } catch {
      status.textContent = "Couldn’t load players. ";
      const retry = document.createElement("button");
      retry.type = "button";
      retry.textContent = "Try again";
      retry.addEventListener("click", browse);
      status.append(retry);
    }
  }
  input.addEventListener("input", render);
  input.addEventListener("keydown", event => {
    if (event.key === "Enter") event.preventDefault();
  });
  panel.addEventListener("toggle", () => { if (panel.open) void browse(); });
  return {panel, reset: () => select(null)};
}

const seats = [...document.querySelectorAll(".new-game-form select[name^=seat]")];
for (const seat of seats) {
  const invite = seat.querySelector('option[value="invite"]');
  const picker = playerPicker("Player for seat " + seat.name.slice(4), player => {
    invite.value = player ? "invite:" + player.id : "invite";
    seat.setCustomValidity(seat.value === "invite" ? "Choose a player to invite." : "");
  }, () => new Set(seats.filter(other => other !== seat && other.value.startsWith("invite:"))
    .map(other => other.value.slice(7))));
  seat.closest("label").after(picker.panel);
  function togglePicker() {
    const enabled = seat.selectedOptions[0] === invite;
    picker.reset();
    picker.panel.hidden = !enabled;
    picker.panel.open = enabled;
  }
  seat.addEventListener("change", togglePicker);
  togglePicker();
}

if (inviteForm) {
  const send = inviteForm.querySelector("button");
  const picker = playerPicker("Player to invite", player => {
    inviteForm.elements.player.value = player?.id || "";
    send.disabled = !player;
  }, () => new Set());
  send.disabled = true;
  inviteForm.prepend(picker.panel);
  picker.panel.open = true;
}

setupInstallGuide(document.querySelector("#install-guide"));

const inbox = document.querySelector("#invitations");
if (inbox || document.querySelector(".back-link")) {
  const alertLink = document.createElement("a");
  alertLink.className = "invitation-alert button";
  alertLink.href = "/#invitations";
  alertLink.textContent = "View game invitations";
  const chatHeading = document.querySelector(".chat-heading");
  const previous = new Map();
  let stream;
  let request;
  let queued = false;
  let stopped = false;
  let failures = 0;
  const connection = document.createElement("a");
  connection.className = "activity-reconnect";
  connection.href = location.href;
  connection.textContent = "Updates paused · Reconnect";
  function reportFailure() {
    if (++failures >= 2 && !connection.isConnected) {
      if (chatHeading) chatHeading.append(connection);
      else document.querySelector(".nav").after(connection);
    }
  }
  function replace(id, html) {
    if (previous.get(id) === html) return;
    previous.set(id, html);
    const element = document.getElementById(id);
    const finishedExpanded = element.querySelector("details[open]") != null;
    element.innerHTML = html;
    const details = element.querySelector("details");
    if (finishedExpanded && details) details.open = true;
  }
  async function refresh() {
    if (document.hidden || stopped) return;
    if (request) { queued = true; return; }
    const controller = new AbortController();
    request = controller;
    const timeout = setTimeout(() => controller.abort(), 8000);
    try {
      const response = await fetch(inbox ? "/api/activity" : "/api/invitations", {
        cache: "no-store", signal: controller.signal,
      });
      if (response.status === 401) {
        failures = 2;
        reportFailure();
        stopped = true;
        stream?.close();
        return;
      }
      if (!response.ok || response.redirected) { reportFailure(); return; }
      if (inbox) {
        const activity = await response.json();
        const panel = document.getElementById("phone-alerts");
        if (panel && panel.dataset.away !== String(activity.away === true)) {
          panel.dataset.away = String(activity.away === true);
          panel.dispatchEvent(new Event("awaychange"));
        }
        replace("invitations", activity.invitations);
        replace("my-games", activity.games);
        replace("open-games", activity.open_games);
      } else {
        const html = await response.text();
        const away = response.headers.get("x-scrabble-away");
        if (away !== null && document.querySelector(".away-settings")) {
          window.dispatchEvent(new CustomEvent("scrabble-away", {detail: away === "true"}));
        }
        const showAlert = html && away !== "true";
        if (showAlert && !alertLink.isConnected) (chatHeading || document.body).append(alertLink);
        else if (!showAlert) alertLink.remove();
      }
      failures = 0;
      connection.remove();
    } catch { if (!document.hidden) reportFailure(); }
    finally {
      clearTimeout(timeout);
      request = null;
      if (queued) { queued = false; void refresh(); }
    }
  }
  function pause() {
    stream?.close();
    stream = null;
    request?.abort();
  }
  function resume() {
    if (document.hidden || stopped) return;
    if (!stream && "EventSource" in window) {
      stream = new EventSource("/api/activity/events");
      stream.addEventListener("open", refresh);
      stream.addEventListener("message", refresh);
    }
    void refresh();
  }
  // Phone browsers suspend connections and timers. Always read current state on return.
  document.addEventListener("visibilitychange", () => document.hidden ? pause() : resume());
  window.addEventListener("pagehide", pause);
  window.addEventListener("pageshow", resume);
  window.addEventListener("focus", resume);
  window.addEventListener("online", resume);
  navigator.serviceWorker?.addEventListener("message", event => {
    if (event.data?.type === "scrabble-activity") void refresh();
  });
  setInterval(refresh, 5000);
  resume();
}
