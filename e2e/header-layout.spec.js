import {expect, test} from "@playwright/test";
import {randomUUID} from "node:crypto";

function headerLayout(header) {
  const strip = header.querySelector(".header-scores");
  const area = strip.getBoundingClientRect();
  const targets = [...header.querySelectorAll("a")].map(link => link.getBoundingClientRect());
  const values = [...header.querySelectorAll(".header-score-value")].map(value => value.getBoundingClientRect());
  return {
    actions: header.querySelectorAll(".nav-links > :is(a, button)").length,
    scoreAndBackShareRow: area.top < header.querySelector(".back-link").getBoundingClientRect().bottom,
    font: parseFloat(getComputedStyle(strip).fontSize),
    scoresVisible: values.every(r => r.left >= area.left - 1 && r.right <= area.right + 1
      && r.top >= area.top - 1 && r.bottom <= area.bottom + 1),
    distinctTargets: targets.every(r => r.height >= 44 && r.left >= 0 && r.right <= innerWidth + 1
      && values.every(v => v.right <= r.left || v.left >= r.right || v.bottom <= r.top || v.top >= r.bottom)),
    overflow: document.documentElement.scrollWidth - innerWidth,
  };
}

async function openHeaderGame(page, seats, browser, baseURL) {
  if (seats === 2) {
    await page.request.post("/auth/login/begin", {data:{username:"shelli"}});
    return "/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6";
  }

  const guest = await browser.newContext({baseURL});
  try {
    const suffix = randomUUID().slice(0, 8);
    const recipient = "guest-" + suffix;
    await guest.request.post("/auth/register/begin", {data:{username:recipient, display_name:recipient}});
    await page.request.post("/auth/register/begin", {data:{username:"host-" + suffix, display_name:"A very long player name"}});
    const [player] = await (await page.request.get("/api/players?q=" + recipient)).json();
    // Reserve the human seat so this layout fixture never appears in other players' open games.
    const response = await page.request.post("/games", {
      form:{seat2:"invite:" + player.id, seat3:"hard", seat4:"medium"}, maxRedirects:0,
    });
    expect(response.status()).toBe(303);
    return response.headers().location;
  } finally { await guest.close(); }
}

for (const width of [320, 390]) {
  for (const seats of [2, 4]) {
    test(`${seats}-seat phone header fits scores and navigation at ${width}px`, async ({page, browser, baseURL}) => {
      await page.setViewportSize({width, height:700});
      await page.goto(await openHeaderGame(page, seats, browser, baseURL));
      await expect(page.locator("body > .status-bar-edge")).toHaveAttribute("aria-hidden", "true");
      await expect(page.locator(".status-bar-edge")).toBeHidden();
      const nav = page.locator(".game-page .nav");
      await expect(nav.locator(".header-score-value")).toHaveCount(seats);
      await expect(nav.getByRole("link", {name:"Chat", exact:true})).toBeVisible();
      const layout = await nav.evaluate(headerLayout);
      if (seats === 2 && layout.actions <= 2) expect(layout.scoreAndBackShareRow).toBe(true);
      expect(layout.font).toBeGreaterThanOrEqual(13);
      expect(layout.scoresVisible).toBe(true);
      expect(layout.distinctTargets).toBe(true);
      expect(layout.overflow).toBeLessThanOrEqual(1);
    });
  }
}

test("the home rack and sign-in form fit a narrow phone with large text", async ({page}) => {
  await page.setViewportSize({width:320, height:693});
  await page.goto("/");
  await page.evaluate(() => { document.documentElement.style.fontSize = "200%"; });
  const home = page.getByRole("link", {name:"Scrabble home", exact:true});
  await expect(home.locator(".brand-tile")).toHaveText(["S1","C3","R1","A1","B3","B3","L1","E1"]);
  const bounds = await home.boundingBox();
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(320);
  expect(bounds.height).toBeGreaterThanOrEqual(44);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(320);
});
