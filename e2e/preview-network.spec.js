import { test, expect } from "@playwright/test";

const board = "/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6";

test("an old draft reply cannot replace the latest word check; unavailable preview leaves Play available", async ({page}) => {
  await page.request.post("/auth/login/begin", {data:{username:"shelli"}});
  // Deliberately model a transport that ignores aborts, so the first result
  // can arrive after the replacement draft has already been validated.
  await page.addInitScript(() => {
    const fetch = window.fetch.bind(window);
    window.fetch = (url, options) => {
      if (!String(url).endsWith("/preview") || options?.method !== "POST") return fetch(url, options);
      const count = JSON.parse(options.body).placements.length;
      if (count === 1) return new Promise(resolve => {
        window.finishOldDraft = () => resolve(Response.json({
          valid: true, points: 900, words: [{word:"OLD", valid:true, points:900}], error:null,
        }));
      });
      if (count === 2) return Response.json({
        valid: false, points: null, words:[{word:"FV",valid:false,points:0}], error:"not in dictionary: FV",
      });
      return Promise.reject(new Error("Test preview offline"));
    };
  });
  await page.goto(board);
  const tile = name => page.locator(".rack button").filter({
    has:page.locator(".tile-letter").filter({hasText:new RegExp("^" + name + "$")}),
  }).first();
  await expect(tile("F")).toBeVisible();
  await page.locator('.board:visible .cell[data-row="7"][data-col="7"]').click();
  await tile("F").click();
  await expect.poll(() => page.evaluate(() => typeof window.finishOldDraft)).toBe("function");
  const controls = page.locator(".rack-area .controls");
  const controlsTop = () => controls.evaluate(el => el.getBoundingClientRect().top - el.parentElement.getBoundingClientRect().top);
  const checkingTop = await controlsTop();
  await expect(page.locator(".draft-review")).toHaveText("Checking…");
  await tile("V").click();
  const review = page.locator(".draft-review");
  const play = page.locator(".play-button");
  await expect(review).toHaveAttribute("data-state", "invalid");
  await expect(play).toBeDisabled();
  await page.evaluate(async () => {
    window.finishOldDraft();
    await new Promise(requestAnimationFrame);
  });
  await expect(review.locator(".draft-word.is-invalid")).toHaveText(/FV/);
  await expect(review.locator(".draft-total")).toHaveCount(0);
  await tile("A").click();
  await expect(review).toHaveAttribute("data-state", "unavailable");
  await expect(play).toBeEnabled();
  expect(Math.abs(await controlsTop() - checkingTop)).toBeLessThanOrEqual(1);
  // The authoritative submission is still possible with a failed preview.
  let submitted;
  await page.route("**/move", route => {
    submitted = route.request().postDataJSON();
    return route.fulfill({status:400,json:{error:"Cannot submit fixture play"}});
  });
  await play.click();
  await expect(page.getByRole("alert")).toContainText("Cannot submit fixture play");
  expect(Math.abs(await controlsTop() - checkingTop)).toBeLessThanOrEqual(1);
  expect(await page.getByRole("alert").evaluate(el => el.getBoundingClientRect().top >= el.parentElement.querySelector(".controls").getBoundingClientRect().bottom)).toBe(true);
  await expect(review).toHaveCount(0);
  expect(submitted.placements.map(p => p.letter)).toEqual(["F", "V", "A"]);
  await page.getByRole("button", {name:"Clear",exact:true}).click();
  await expect(review).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a mixed cross-word check names the rejected words and keeps the valid breakdown optional", async ({page}) => {
  await page.request.post("/auth/login/begin", {data:{username:"shelli"}});
  await page.route("**/preview", route => {
    const count = route.request().postDataJSON().placements.length;
    const result = count === 1 ? {
      valid:false, points:null, error:"not allowed in Grandpa Mode: QI",
      words:[{word:"QI",valid:false,points:11},{word:"BH",valid:false,points:7},{word:"CAT",valid:true,points:5}],
    } : {
      valid:true, points:12, error:null, words:[{word:"FAR",valid:true,points:8},{word:"AT",valid:true,points:4}],
    };
    return route.fulfill({json:result});
  });
  await page.goto(board);
  const rack = page.locator(".rack");
  const tile = letter => rack.locator("button").filter({has:page.getByText(letter, {exact:true})}).first();
  await expect(tile("F")).toBeVisible();
  await page.locator('.board:visible .cell[data-row="7"][data-col="7"]').click();
  await tile("F").click();
  const review = page.locator(".draft-review");
  await expect(review.locator(".draft-reason")).toHaveText(["QI isn’t allowed in Grandpa Mode.", "BH isn’t in the dictionary."]);
  await tile("A").click();
  await expect(review.locator("summary")).toHaveText("FAR, AT · 12 pts");
  const breakdown = review.locator(".draft-breakdown");
  await expect(breakdown).toBeHidden();
  await review.locator("summary").click();
  await expect(breakdown).toHaveText("FAR 8 · AT 4");
});
