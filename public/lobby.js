// Keep filters and search outside the live fragments so typing and keyboard focus survive updates.
const lobby = document.querySelector(".lobby");
if (lobby) {
  const historyView = lobby.dataset.view === "History";
  const input = lobby.querySelector("input[type=search]");
  const options = lobby.querySelector("#lobby-options");
  const filterButton = lobby.querySelector("#lobby-filters");
  const tabs = [...lobby.querySelectorAll("[data-tab]")];
  const empty = lobby.querySelector(".lobby-empty");
  const inbox = lobby.querySelector("#invitations");
  const results = lobby.querySelector("#my-games");
  const open = lobby.querySelector("#open-games");
  let tab = tabs[0].dataset.tab;
  function resetOptions() {
    for (const select of options?.querySelectorAll("select") || []) select.selectedIndex = 0;
  }
  function render() {
    const query = input.value.trim().toLowerCase();
    const period = Number(options?.querySelector("[name=period]").value || 0);
    const kind = options?.querySelector("[name=kind]").value || "all";
    const sort = options?.querySelector("[name=sort]").value || "newest";
    const filterCount = Number(!!period) + Number(kind !== "all") + Number(sort !== "newest");
    if (filterButton) filterButton.querySelector("span").textContent = "Filters" + (filterCount ? " · " + filterCount : "");
    for (const button of tabs) button.setAttribute("aria-pressed", String(button.dataset.tab === tab));
    const cutoff = Date.now() / 1000 - period * 86400;
    open.hidden = tab !== "open";
    results.hidden = tab === "open";
    inbox.hidden = historyView || tab !== "mine";
    let count = 0;
    let invitationsShown = false;
    for (const invite of inbox.querySelectorAll(".invite-row")) {
      invite.hidden = !invite.textContent.toLowerCase().includes(query);
      if (!invite.hidden) invitationsShown = true;
    }
    if (!invitationsShown) inbox.hidden = true;
    for (const section of lobby.querySelectorAll("[data-group]")) {
      const group = section.dataset.group;
      const enabled = historyView || (tab === "open" ? group === "open" : group !== "open" && (tab === "mine" || group === "turn"));
      const list = section.querySelector(".lobby-list");
      const rows = [...list.children];
      if (historyView) {
        rows.sort((a,b) => {
          if (sort === "closest" && a.dataset.gap !== b.dataset.gap) return +a.dataset.gap - +b.dataset.gap;
          if (sort === "score" && a.dataset.score !== b.dataset.score) return +b.dataset.score - +a.dataset.score;
          return sort === "oldest" ? +a.dataset.time - +b.dataset.time : +b.dataset.time - +a.dataset.time;
        });
        // Moving even an already-sorted focused link can blur it.
        if (rows.some((row, index) => row !== list.children[index])) {
          const focused = list.contains(document.activeElement) ? document.activeElement : null;
          list.append(...rows);
          focused?.focus({preventScroll:true});
        }
      }
      let shown = 0;
      for (const row of rows) {
        const d = row.dataset;
        const fitsHistory = !historyView || ((tab === "all" || tab === d.result)
          && (!period || +d.time >= cutoff)
          && (kind === "all" || (kind === "bots") === (d.bot === "true")));
        row.hidden = !enabled || !fitsHistory || !d.search.toLowerCase().includes(query);
        if (!row.hidden) shown++;
      }
      section.hidden = !shown;
      section.querySelector(".lobby-count").textContent = String(shown);
      count += shown;
    }
    empty.hidden = count > 0 || !inbox.hidden;
    const hasFilters = !!period || kind !== "all";
    const filtered = !!query || hasFilters;
    empty.querySelector("h2").textContent = filtered ? "No matching games" :
      tab === "turn" ? "You’re all caught up." : tab === "open" ? "No open games." :
      historyView ? (tab === "all" ? "No finished games yet." : "No matching games") : "No games yet.";
    const reset = empty.querySelector(".lobby-reset");
    reset.hidden = !filtered;
    reset.textContent = query && hasFilters ? "Clear search and filters" : query ? "Clear search" : "Reset filters";
    for (const link of empty.querySelectorAll("[data-empty-tab]")) {
      link.hidden = filtered || link.dataset.emptyTab !== tab;
    }
    lobby.querySelector("#lobby-count").textContent = count + (count === 1 ? " game" : " games");
  }
  input.addEventListener("input", render);
  lobby.querySelector(".lobby-tabs").addEventListener("click", event => {
    const button = event.target.closest("[data-tab]");
    if (button) { tab = button.dataset.tab; render(); }
  });
  filterButton?.addEventListener("click", () => {
    options.hidden = !options.hidden;
    filterButton.setAttribute("aria-expanded", String(!options.hidden));
  });
  options?.addEventListener("change", render);
  options?.querySelector(".lobby-reset").addEventListener("click", () => { resetOptions(); render(); });
  empty.querySelector(".lobby-reset").addEventListener("click", () => {
    input.value = "";
    for (const select of options?.querySelectorAll("select:not([name=sort])") || []) select.selectedIndex = 0;
    render();
    input.focus();
  });
  function openNotice(hash) {
    if (historyView || !["#invitations", "#my-games"].includes(hash)) return;
    input.value = "";
    tab = hash === "#invitations" ? "mine" : "turn";
    render();
    const destination = hash === "#invitations" ? inbox : results;
    destination.focus({preventScroll:true});
    destination.scrollIntoView({block:"nearest"});
  }
  document.addEventListener("click", event => {
    const notice = event.target.closest("a.nav-notice");
    if (notice && !historyView && !event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey) {
      openNotice(notice.hash);
    }
  });
  window.addEventListener("hashchange", () => openNotice(location.hash));
  lobby.addEventListener("lobbyrefresh", render);
  window.addEventListener("pageshow", render);
  render();
  openNotice(location.hash);
}
