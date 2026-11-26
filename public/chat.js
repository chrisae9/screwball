const room = document.querySelector("[data-chat-game]");
const url = "/api/games/" + room.dataset.chatGame + "/chat";
const history = room.querySelector(".chat-history");
const list = room.querySelector(".chat-messages");
const intro = room.querySelector(".chat-intro");
const older = room.querySelector(".chat-older");
const status = room.querySelector(".chat-status");
const form = room.querySelector("form");
const input = room.querySelector("textarea");
const send = form.querySelector("button");
const date = new Intl.DateTimeFormat(undefined, {
  month: "short", day: "numeric", hour: "numeric", minute: "2-digit",
});
let first = 0;
let last = 0;
let polling = false;
let retry;
let readError = false;

// A fetched page can contain offscreen messages or skip earlier pages. Only
// acknowledge rows that reached the visible conversation on this device.
const unseenRows = new Set();
const seen = new Set();
let savingReads = false;
let readTimer;
async function saveReads() {
  clearTimeout(readTimer);
  if (savingReads || !seen.size) return;
  savingReads = true;
  const batch = [...seen].slice(0, 100);
  try {
    const response = await fetch(url + "/read", {
      method: "POST", headers: {"Content-Type": "application/json"},
      body: JSON.stringify({seen: batch}), keepalive: true, signal: AbortSignal.timeout(8000),
    });
    if (response.ok) {
      for (const seq of batch) seen.delete(seq);
      if (seen.size) readTimer = setTimeout(saveReads, 0);
    }
  } catch { /* Visible chat polling retries a receipt after reconnect. */ }
  finally { savingReads = false; }
}

function recordVisible() {
  if (document.hidden) return;
  const bounds = history.getBoundingClientRect();
  const viewport = window.visualViewport;
  const top = Math.max(bounds.top, viewport?.offsetTop ?? 0);
  const bottom = Math.min(bounds.bottom, (viewport?.offsetTop ?? 0) + (viewport?.height ?? innerHeight));
  for (const item of unseenRows) {
    const rect = item.getBoundingClientRect();
    if (!item.isConnected || rect.height === 0 || rect.bottom <= top || rect.top >= bottom) continue;
    seen.add(Number(item.dataset.seq));
    unseenRows.delete(item);
  }
  if (seen.size) {
    clearTimeout(readTimer);
    readTimer = setTimeout(saveReads, 80);
  }
}
let readFrame;
function scheduleVisible() {
  if (readFrame) return;
  readFrame = requestAnimationFrame(() => { readFrame = 0; recordVisible(); });
}
history.addEventListener("scroll", scheduleVisible, {passive: true});
window.visualViewport?.addEventListener("resize", scheduleVisible);
window.visualViewport?.addEventListener("scroll", scheduleVisible);
document.addEventListener("visibilitychange", recordVisible);
window.addEventListener("pagehide", saveReads);
document.querySelector(".back-link")?.addEventListener("click", saveReads);

function row(message) {
  const item = document.createElement("li");
  if (message.mine) item.classList.add("mine");
  else {
    item.dataset.seq = String(message.seq);
    unseenRows.add(item);
  }
  const meta = document.createElement("div");
  meta.className = "chat-meta";
  const author = document.createElement("strong");
  author.textContent = message.mine ? "You" : message.name;
  const time = document.createElement("time");
  time.dateTime = message.sent_at;
  time.textContent = date.format(new Date(message.sent_at));
  meta.append(author, time);
  const text = document.createElement("p");
  text.className = "chat-bubble";
  text.dir = "auto";
  text.textContent = message.text;
  item.append(meta, text);
  return item;
}

async function get(query) {
  const response = await fetch(url + query, {cache: "no-store", signal: AbortSignal.timeout(8000)});
  if (!response.ok) throw new Error("Unable to load messages.");
  return response.json();
}

async function refresh() {
  if (document.hidden || polling) return;
  polling = true;
  try {
    const page = await get(last ? "?after=" + last : "");
    const follow = !last || history.scrollHeight - history.scrollTop - history.clientHeight < 64;
    if (!first && page.messages.length) {
      first = page.messages[0].seq;
      older.hidden = !page.more_before;
    }
    for (const message of page.messages) {
      list.append(row(message));
      last = message.seq;
    }
    intro.hidden = !!last;
    if (!last) intro.textContent = "Start the conversation with the players in this game.";
    if (follow) history.scrollTop = history.scrollHeight;
    if (readError) status.textContent = "";
    readError = false;
    recordVisible();
    if (page.messages.length) void saveReads();
    if (page.messages.length === 50) queueMicrotask(refresh);
  } catch {
    if (!last) intro.textContent = "Waiting for a connection…";
    if (!retry) {
      status.textContent = "Couldn’t load messages. Reconnecting…";
      readError = true;
    }
  } finally {
    polling = false;
  }
}
older.addEventListener("click", async () => {
  older.disabled = true;
  try {
    const page = await get("?before=" + first);
    const height = history.scrollHeight;
    list.prepend(...page.messages.map(row));
    if (page.messages.length) first = page.messages[0].seq;
    older.hidden = !page.more_before;
    history.scrollTop += history.scrollHeight - height;
    scheduleVisible();
  } catch {
    status.textContent = "Couldn’t load earlier messages. Try again.";
  } finally {
    older.disabled = false;
  }
});

form.addEventListener("submit", async event => {
  event.preventDefault();
  const text = input.value.trim();
  if (!text || send.disabled) return;
  if (retry?.text !== text) retry = { id: crypto.randomUUID(), text };
  send.disabled = true;
  status.textContent = "";
  try {
    const response = await fetch(url, {
      method: "POST", headers: {"Content-Type": "application/json"},
      body: JSON.stringify(retry), signal: AbortSignal.timeout(10000),
    });
    if (!response.ok) throw new Error("Unable to send.");
    if (input.value.trim() === text) input.value = "";
    retry = null;
    history.scrollTop = history.scrollHeight;
    await refresh();
  } catch {
    status.textContent = "Couldn’t confirm delivery. Tap Send to retry.";
  } finally {
    send.disabled = false;
  }
});

// iOS overlays its keyboard on the layout viewport. Size the conversation to
// the visible part so the composer stays reachable without covering messages.
function resize() {
  if (window.visualViewport?.scale === 1) {
    const viewport = window.visualViewport;
    const nextHeight = viewport.height + "px";
    const heightChanged = nextHeight !== document.body.style.getPropertyValue("--chat-viewport");
    const following = history.scrollHeight - history.scrollTop - history.clientHeight < 64;
    document.body.classList.toggle("chat-compact", viewport.height < 360);
    document.body.style.setProperty("--chat-viewport", nextHeight);
    if (heightChanged && following) requestAnimationFrame(() => { history.scrollTop = history.scrollHeight; });
    // Safari also pans the page to the focused text box when the keyboard opens.
    document.body.style.setProperty("--chat-top", viewport.pageTop + "px");
  }
}
window.visualViewport?.addEventListener("resize", resize);
window.visualViewport?.addEventListener("scroll", resize);
resize();
document.addEventListener("visibilitychange", refresh);
window.addEventListener("pageshow", refresh);
window.addEventListener("online", refresh);
setInterval(refresh, 3000);
refresh();
