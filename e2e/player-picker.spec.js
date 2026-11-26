import { expect, test } from "@playwright/test";

test("browse beyond the first dozen, refine immediately and choose distinct opponents", async ({ page }) => {
  const players = Array.from({ length: 24 }, (_, i) => ({ id: String(i), name: "Player " + String(i + 1).padStart(2, "0") }));
  players.push({ id: "z", name: "Zoë" });
  await page.route("**/api/players", route => route.fulfill({json: players}));
  await page.request.post("/auth/login/begin", {data: {username: "scott"}});
  await page.goto("/games/new");
  const panel = page.locator(".player-picker[open]");
  const rows = panel.getByRole("button");
  await expect(rows).toHaveCount(25);
  expect(await panel.locator("ul").evaluate(el => el.scrollHeight > el.clientHeight)).toBe(true);
  expect(await page.evaluate(() => document.activeElement.tagName)).not.toBe("INPUT");
  await rows.last().scrollIntoViewIfNeeded();
  expect(await panel.locator("ul").evaluate(el => el.scrollTop)).toBeGreaterThan(0);
  const filter = panel.getByRole("searchbox");
  await filter.fill("z");
  await expect(rows).toHaveCount(1);
  await expect(rows).toHaveAccessibleName("Zoë");
  await filter.fill("absent");
  await expect(panel.getByRole("status")).toHaveText("No matching players.");
  await filter.fill("");
  await panel.getByRole("button", {name:"Player 24", exact:true}).click();
  await expect(page.locator('[name="seat2"]')).toHaveValue("invite:23");
  await page.locator('[name="seat3"]').selectOption("invite");
  await expect(panel.getByRole("searchbox")).toHaveAccessibleName("Player for seat 3");
  await expect(rows).toHaveCount(24);
  await expect(panel.getByRole("button", {name:"Player 24", exact:true})).toHaveCount(0);
  await panel.getByRole("button", {name:"Zoë", exact:true}).click();
  await expect(page.locator('[name="seat3"]')).toHaveValue("invite:z");
  await page.locator('[name="seat2"]').selectOption("easy");
  await page.getByText("Change", {exact:true}).click();
  await expect(panel.getByRole("button", {name:"Player 24", exact:true})).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test("existing game host gets an immediate roster and can select a recipient", async ({ page }) => {
  await page.request.post("/auth/login/begin", {data: {username: "shelli"}});
  const path = "/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6/invite";
  await page.goto(path);
  const list = page.getByRole("list", {name:"Available players"});
  await expect(list.getByRole("button", {name:"Shelli", exact:true})).toHaveCount(0);
  const send = page.getByRole("button", {name:"Send invitation"});
  await expect(send).toBeDisabled();
  await list.getByRole("button", {name:"Scott", exact:true}).click();
  await expect(send).toBeEnabled();
  await expect(page.getByRole("link", {name:"Back to board", exact:true})).toHaveAttribute("href", path.replace(/\/invite$/, ""));
});
