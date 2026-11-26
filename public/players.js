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

function playerPicker(container, labelText, onChoose) {
  const label = document.createElement("label");
  label.textContent = labelText;
  const input = document.createElement("input");
  input.type = "search";
  input.placeholder = "Search by player name";
  input.autocomplete = "off";
  input.required = true;
  label.append(input);
  const results = document.createElement("div");
  results.className = "player-results";
  const status = document.createElement("small");
  status.setAttribute("role", "status");
  container.append(label, results, status);
  let timer;
  let generation = 0;
  input.addEventListener("input", () => {
    const version = ++generation;
    clearTimeout(timer);
    onChoose(null);
    input.setCustomValidity("Select a player from the search results.");
    results.replaceChildren();
    const query = input.value.trim();
    status.textContent = query.length < 2 ? "Enter at least two letters." : "Searching…";
    if (query.length < 2) return;
    timer = setTimeout(async () => {
      try {
        const response = await fetch("/api/players?q=" + encodeURIComponent(query));
        if (!response.ok) throw new Error();
        const players = await response.json();
        if (version !== generation) return;
        status.textContent = players.length ? "Choose a player." : "No matching players.";
        for (const player of players) {
          const choice = document.createElement("button");
          choice.type = "button";
          choice.className = "button button-secondary";
          const ambiguous = players.some(p => p.id !== player.id && p.name === player.name);
          choice.textContent = ambiguous ? player.name + " · " + player.id.slice(0, 6) : player.name;
          choice.addEventListener("click", () => {
            input.value = player.name;
            input.setCustomValidity("");
            onChoose(player);
            results.replaceChildren();
            status.textContent = player.name + " selected.";
          });
          results.append(choice);
        }
      } catch {
        if (version === generation) status.textContent = "Search failed. Try typing the name again.";
      }
    }, 160);
  });
}

for (const seat of document.querySelectorAll(".new-game-form select[name^=seat]")) {
  const field = document.createElement("div");
  field.className = "player-picker";
  seat.closest("label").after(field);
  const invite = seat.querySelector('option[value="invite"]');
  playerPicker(field, "Player for seat " + seat.name.slice(4), player => {
    invite.value = player ? "invite:" + player.id : "invite";
  });
  function togglePicker() {
    const enabled = seat.selectedOptions[0] === invite;
    field.hidden = !enabled;
    field.querySelector("input").disabled = !enabled;
  }
  seat.addEventListener("change", togglePicker);
  togglePicker();
}

const inviteForm = document.querySelector("[data-invite-form]");
if (inviteForm) {
  const back = document.querySelector(".back-link");
  back.href = inviteForm.action.replace(/\/invite$/, "");
  back.setAttribute("aria-label", "Back to game");
  const picker = document.createElement("div");
  inviteForm.prepend(picker);
  playerPicker(picker, "Player to invite", player => {
    inviteForm.elements.player.value = player ? player.id : "";
  });
}

setupInstallGuide(document.querySelector("#install-guide"));

const inbox = document.querySelector("#invitations");
if (inbox || document.querySelector(".back-link")) {
  const alertLink = document.createElement("a");
  alertLink.className = "invitation-alert button";
  alertLink.href = "/#invitations";
  alertLink.textContent = "View game invitations";
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
    if (++failures >= 2 && !connection.isConnected) document.querySelector(".nav").after(connection);
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
        replace("invitations", activity.invitations);
        replace("my-games", activity.games);
        replace("open-games", activity.open_games);
      } else {
        const html = await response.text();
        if (html && !alertLink.isConnected) document.body.append(alertLink);
        else if (!html) alertLink.remove();
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
