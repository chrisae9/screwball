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

function row(message) {
  const item = document.createElement("li");
  if (message.mine) item.classList.add("mine");
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
    document.body.style.setProperty("--chat-viewport", viewport.height + "px");
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
