import { test, expect } from "@playwright/test";

const fixture = "/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6";

// This fixture has an empty board and Shelli holds F, V, R, E, a blank, U and A.
// Use the real board, rack and preview route. Leaving the move unsubmitted keeps
// the shared fixture available to parallel desktop and mobile runs.
test("a player can check a word, correct it and return from chat with the draft intact", async ({ page }) => {
  await page.request.post("/auth/login/begin", {data: {username: "shelli"}});
  await page.goto(fixture);
  const center = page.locator('.board:visible .cell[data-row="7"][data-col="7"]');
  const tile = letter => page.locator(".rack button").filter({
    has: page.locator(".tile-letter").filter({hasText: new RegExp("^" + letter + "$")}),
  }).first();
  const review = page.locator(".draft-review");
  const play = page.locator(".play-button");
  const clear = page.getByRole("button", {name: "Clear", exact: true});
  const chat = page.getByRole("link", {name: /chat/i});

  const controls = page.locator(".rack-area .controls");
  const controlsTop = () => controls.evaluate(el => el.getBoundingClientRect().top - el.parentElement.getBoundingClientRect().top);
  await expect(tile("F")).toBeVisible();
  await expect(chat.locator("svg")).toBeVisible();
  const target = await chat.boundingBox();
  expect(target.width).toBeGreaterThanOrEqual(44);
  expect(target.height).toBeGreaterThanOrEqual(44);
  const restingTop = await controlsTop();
  await center.click();
  await tile("F").click();
  await tile("V").click();
  await expect(review).toHaveAttribute("data-state", "invalid");
  await expect(review.locator(".draft-word.is-invalid")).toContainText("FV");
  await expect(play).toBeDisabled();
  await expect(review).toHaveText("FV isn’t in the dictionary.");
  expect(Math.abs(await controlsTop() - restingTop)).toBeLessThanOrEqual(1);
  expect(await review.evaluate(el => el.getBoundingClientRect().top >= el.parentElement.querySelector(".controls").getBoundingClientRect().bottom)).toBe(true);

  await clear.click();
  await expect(review).toHaveCount(0);
  expect(Math.abs(await controlsTop() - restingTop)).toBeLessThanOrEqual(1);
  await center.click();
  await tile("F").click();
  await tile("A").click();
  await tile("R").click();
  await expect(review).toHaveAttribute("data-state", "valid");
  await expect(review.locator(".draft-word")).toContainText("FAR");
  await expect(review.locator(".draft-total")).toContainText("12");
  await expect(play).toBeEnabled();
  await expect(review).toHaveText("FAR · 12 pts");
  expect(Math.abs(await controlsTop() - restingTop)).toBeLessThanOrEqual(1);
  await Promise.all([page.waitForURL("**" + fixture + "/chat", {waitUntil:"domcontentloaded"}), chat.click()]);
  await expect(page.getByRole("heading", {name: "Game chat"})).toBeVisible();
  await Promise.all([
    page.waitForURL("**" + fixture, {waitUntil:"domcontentloaded"}),
    page.getByRole("link", {name: "Back to board"}).click(),
  ]);
  await expect(tile("V")).toBeVisible();
  await expect(page.locator(".pending-cell")).toHaveCount(3);
  await expect(review).toHaveAttribute("data-state", "valid");
  await expect(review.locator(".draft-total")).toContainText("12");
  const state = await (await page.request.get(fixture + "/state")).json();
  expect(state.seats[0].score).toBe(0);
  expect(state.moves).toHaveLength(0);
});

for (const colorScheme of ["dark", "light"]) {
  test(`blank letter choices keep their tile ink in the ${colorScheme} palette`, async ({ page, isMobile }) => {
    await page.request.post("/auth/login/begin", {data: {username: "shelli"}});
    await page.goto(fixture);
    await page.locator('.board:visible .cell[data-row="7"][data-col="7"]').click();
    await page.locator(".rack button.tile-blank").click();
    const dialog = page.getByRole("dialog", {name: "Choose a letter"});
    const letter = dialog.getByRole("button", {name: "Q", exact: true});
    const ink = await page.locator(".rack button:not(.tile-blank) .tile-letter").first()
      .evaluate(el => getComputedStyle(el).color);
    await page.emulateMedia({colorScheme});
    await expect(letter).toHaveCSS("color", ink);
    if (!isMobile) {
      await letter.hover();
      await expect(letter).toHaveCSS("color", ink);
      await page.mouse.move(0, 0);
    }
    await letter.click();
    await expect(dialog).toHaveCount(0);
    await expect(page.locator(".pending-cell .tile-letter")).toHaveText("Q");
  });
}
