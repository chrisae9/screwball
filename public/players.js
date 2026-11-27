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

function playerAvatar(avatarId) {
  const id = Number.isInteger(avatarId) && avatarId >= 0 && avatarId < 50 ? avatarId : 0;
  const icon = document.createElement("span");
  icon.className = "player-avatar";
  icon.setAttribute("aria-hidden", "true");
  icon.dataset.avatarId = String(id);
  icon.style.setProperty("--avatar-col", String(id % 10));
  icon.style.setProperty("--avatar-row", String(Math.floor(id / 10)));
  return icon;
}

function playerPicker(label, choose, otherSeats) {
  const panel = document.createElement("details");
  panel.className = "player-picker";
  panel.innerHTML = `<summary><span class="picker-title">Choose a player</span><span class="picker-action"></span></summary>
    <div class="picker-content"><input type="search" name="q" form="player-directory-search" placeholder="Find players"
      autocomplete="off" autocapitalize="none" spellcheck="false" enterkeyhint="search">
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
    summary.querySelector(".player-avatar")?.remove();
    if (player) summary.prepend(playerAvatar(player.avatar_id));
    title.textContent = player ? (player.handle ? "@" + player.handle : player.name) : "Choose a player";
    action.textContent = player ? "Change" : "";
    choose(player);
  }

  function render() {
    const query = input.value.trim().toLowerCase();
    const excluded = otherSeats();
    const rows = [];
    for (const player of players) {
      const matches = player.name.toLowerCase().includes(query)
        || (player.handle && ("@" + player.handle).includes(query));
      if (excluded.has(player.id) || !matches) continue;
      const row = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.className = "picker-choice";
      const name = document.createElement("span");
      if (player.handle) {
        name.textContent = "@" + player.handle;
        const displayName = document.createElement("small");
        displayName.textContent = player.name;
        name.append(displayName);
      } else {
        name.textContent = counts.get(player.name) > 1 ? player.name + " · " + player.id.slice(0, 6) : player.name;
      }
      button.append(playerAvatar(player.avatar_id), name);
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
    if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      input.blur();
    }
  });
  panel.addEventListener("toggle", () => { if (panel.open) void browse(); });
  function refreshChoices() {
    if (panel.open && !input.disabled) render();
  }
  return {panel, reset: () => select(null), refreshChoices};
}

const seats = [...document.querySelectorAll(".new-game-form select[name^=seat]")];
function validateSeats() {
  const empty = seats.every(seat => seat.value === "off");
  for (const seat of seats) {
    let error = "";
    if (empty && seat === seats[0]) error = "Choose at least one opponent.";
    if (seat.value === "invite") error = "Choose a player to invite.";
    seat.setCustomValidity(error);
  }
}
// Give the directory filters their own form owner while keeping each beside its seat.
if (seats.length || inviteForm) {
  const searchForm = document.createElement("form");
  searchForm.id = "player-directory-search";
  searchForm.autocomplete = "off";
  searchForm.hidden = true;
  searchForm.addEventListener("submit", event => event.preventDefault());
  document.body.append(searchForm);
}
for (const seat of seats) {
  const invite = seat.querySelector('option[value="invite"]');
  const picker = playerPicker("Player for seat " + seat.name.slice(4), player => {
    invite.value = player ? "invite:" + player.id : "invite";
    validateSeats();
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
  seat.form.addEventListener("change", event => {
    if (event.target !== seat && seats.includes(event.target)) picker.refreshChoices();
  });
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
const dashboardVersion = document.getElementById("my-games")?.dataset.dashboardVersion;
if (inbox || document.querySelector(".back-link")) {
  const alertLink = document.createElement("a");
  const nav = document.querySelector(".nav-links");
  const board = location.pathname.match(/^\/games\/([a-f0-9-]{36})(?:\/|$)/i)?.[1];
  alertLink.className = "nav-notice";
  alertLink.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M18 9a6 6 0 0 0-12 0c0 5-2 7-3 8h18c-1-1-3-3-3-8ZM10 20a2.5 2.5 0 0 0 4 0"/></svg><span class="notice-count" aria-hidden="true"></span>';
  function showNotice(invitations, turns) {
    const count = invitations + turns;
    if (!count) { alertLink.remove(); return; }
    const label = invitations ? "View game invitations" : "View games waiting for your turn";
    alertLink.href = invitations ? "/#invitations" : "/#my-games";
    alertLink.title = label;
    alertLink.setAttribute("aria-label", label);
    alertLink.lastElementChild.textContent = count > 9 ? "9+" : String(count);
    if (!alertLink.isConnected) nav.append(alertLink);
  }
  const previous = new Map();
  let stream;
  let request;
  let queued = false;
  let stopped = false;
  let failures = 0;
  const connection = document.createElement("button");
  connection.type = "button";
  connection.className = "nav-notice";
  connection.title = "Reconnect game updates";
  connection.setAttribute("aria-label", connection.title);
  connection.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 4v5h5M3.5 9a8.5 8.5 0 1 1 .5 7"/></svg>';
  connection.addEventListener("click", () => stopped ? location.reload() : resume());
  function reportFailure() {
    if (++failures >= 2 && !connection.isConnected) {
      alertLink.remove();
      nav.append(connection);
    }
  }
  function replace(id, html) {
    if (previous.get(id) === html) return;
    previous.set(id, html);
    const element = document.getElementById(id);
    const focused = document.activeElement;
    let focusSelector;
    if (element.contains(focused)) {
      if (focused.matches("a[href]")) focusSelector = "a[href=" + CSS.escape(focused.getAttribute("href")) + "]";
      else if (focused.matches("button") && focused.form) focusSelector = "form[action=" + CSS.escape(focused.form.getAttribute("action")) + "] button";
      else if (focused.matches("summary")) focusSelector = "summary";
    }
    const finishedExpanded = element.querySelector("details[open]") != null;
    element.innerHTML = html;
    const details = element.querySelector("details");
    if (finishedExpanded && details) details.open = true;
    if (focusSelector) element.querySelector(focusSelector)?.focus({preventScroll:true});
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
        headers: dashboardVersion ? {"x-scrabble-dashboard": dashboardVersion} : board ? {"x-scrabble-current-game": board} : {},
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
        if (dashboardVersion && activity.version && activity.version !== dashboardVersion) {
          stopped = true;
          stream?.close();
          location.reload();
          return;
        }
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
        const chat = document.querySelector(".game-chat-link");
        const unread = response.headers.get("x-scrabble-chat-unread");
        if (chat && (unread === "true" || unread === "false") && chat.dataset.unread !== unread) {
          chat.dataset.unread = unread;
          chat.setAttribute("aria-label", unread === "true" ? "Chat, unread messages" : "Chat");
          const status = document.querySelector(".chat-unread-status");
          if (status) status.textContent = unread === "true" ? "Unread game chat messages" : "";
        }
        const away = response.headers.get("x-scrabble-away");
        const state = response.headers.get("x-scrabble-away-state");
        if (state !== null && document.querySelector(".away-settings")) {
          window.dispatchEvent(new CustomEvent("scrabble-away", {detail: state}));
        }
        // Counts stay in the navigation bar while the board and rack retain their space.
        const inviteCount = (html.match(/class="invite-row"/g) || []).length || (html.trim() ? 1 : 0);
        const turns = Number(response.headers.get("x-scrabble-other-turns")) || 0;
        if (board && nav.dataset.otherTurns !== String(turns)) {
          nav.dataset.otherTurns = String(turns);
          nav.dispatchEvent(new Event("otherturns"));
        }
        if (away === "true") alertLink.remove();
        else showNotice(inviteCount, turns);
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
