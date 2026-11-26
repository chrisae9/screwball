import {expect, test} from "@playwright/test";

test("an open legacy dashboard receives styled cards on its next activity poll", async ({page}) => {
  await page.request.post("/auth/login/begin", {data:{username:"shelli"}});
  // Keep the old document and its CSS alive, as an installed phone app does.
  await page.route("**/old-dashboard", route => route.fulfill({
    contentType:"text/html",
    body:`<style>.game-list-item{display:flex}.game-list a{color:orangered}</style>
      <section class="card"><h1>Your games</h1><div id="my-games"></div>
      <p class="debug-link"><a href="/debug/touch">Touch debug</a></p></section>`,
  }));
  await page.goto("/old-dashboard");
  await page.evaluate(async () => {
    const activity = await (await fetch("/api/activity")).json();
    document.getElementById("my-games").innerHTML = activity.games;
  });
  const card = page.locator(".match-card").first();
  await expect(card).toHaveCSS("display", "grid");
  await expect(page.locator(".match-meta").first()).toHaveCSS("display", "flex");
  await expect(page.locator(".debug-link")).toBeHidden();
  expect(await page.locator(".match-player").first().evaluate(row => {
    const [player,score] = row.children;
    return score.getBoundingClientRect().left > player.getBoundingClientRect().right;
  })).toBe(true);
  await expect(page).toHaveURL(/\/old-dashboard$/);
});

test("a new dashboard refreshes its whole page once across releases", async ({page}) => {
  await page.request.post("/auth/login/begin", {data:{username:"shelli"}});
  let documents = 0;
  await page.route(url => url.pathname === "/", async route => {
    documents++;
    if (documents > 1) { await route.continue(); return; }
    const response = await route.fetch();
    const body = (await response.text()).replace(/data-dashboard-version="[^"]+"/, 'data-dashboard-version="previous"');
    await route.fulfill({response,body});
  });
  await page.goto("/");
  await expect.poll(() => documents).toBe(2);
  const games = page.locator("#my-games");
  await expect(games).toHaveAttribute("data-dashboard-version", /^(?!previous$).+/);
  const poll = page.waitForResponse(r => r.url().endsWith("/api/activity") && !!r.request().headers()["x-scrabble-dashboard"]);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  const activity = await (await poll).json();
  expect(activity.version).toBe(await games.getAttribute("data-dashboard-version"));
  expect(activity.games).not.toContain('rel="stylesheet"');
  expect(documents).toBe(2);
});
