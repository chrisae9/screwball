// Apply the saved palette before the first paint, including Home Screen launches.
(() => {
  const key = "scrabble.appearance";
  const systemDark = "(prefers-color-scheme: dark)";
  const palette = document.getElementById("dark-palette");
  const metas = [...document.querySelectorAll('meta[name="theme-color"]')];
  const media = metas.map(meta => meta.media);
  let preference = "system";
  function apply(value) {
    preference = value === "dark" || value === "light" ? value : "system";
    const system = preference === "system";
    palette.media = system ? systemDark : preference === "dark" ? "all" : "not all";
    metas.forEach((meta, i) => {
      meta.media = system ? media[i] : media[i].includes(preference) ? "all" : "not all";
    });
    const input = document.querySelector('.theme-options input[value="' + preference + '"]');
    if (input) input.checked = true;
  }
  function restore() {
    try { apply(localStorage.getItem(key)); }
    catch { apply(preference); }
  }
  restore();
  document.addEventListener("DOMContentLoaded", () => apply(preference));
  document.addEventListener("change", event => {
    if (!event.target.matches('.theme-options input[name="appearance"]')) return;
    apply(event.target.value);
    try { localStorage.setItem(key, preference); } catch { /* Available for this page. */ }
  });
  window.addEventListener("pageshow", restore);
  window.addEventListener("storage", event => { if (event.key === key || event.key === null) restore(); });
})();
