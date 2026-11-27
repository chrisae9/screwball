import { test, expect } from "@playwright/test";
import { randomUUID } from "node:crypto";

test("a player previews a finish, chooses a starter, and manages their collection", async ({ page, browser, baseURL }) => {
  const username = "Tile" + randomUUID().slice(0, 8);
  expect((await page.request.post("/auth/register/begin", { data: { username, display_name: username } })).ok()).toBe(true);
  await page.goto("/");
  await page.locator(".primary-menu").getByRole("link", { name: "Shop" }).click();
  await expect(page.locator(".coin-balance")).toContainText("0 coins");
  await expect(page.locator(".shop-card")).toHaveCount(16);
  await page.locator('a.shop-card[href="/shop?item=cobalt"]').click();
  await expect(page.getByRole("heading", { name: "Cobalt Enamel" })).toBeVisible();
  await expect(page.locator(".mini-board > *")).toHaveCount(15);
  const tile = await page.locator(".mini-board .sample-tile").first().boundingBox();
  expect(Math.abs(tile.width - tile.height)).toBeLessThan(1);
  await page.getByRole("button", { name: "Choose free starter" }).click();
  await expect(page.getByRole("status")).toContainText("Equipped");
  await page.getByRole("link", { name: "‹ Tile Shop" }).click();
  await page.getByRole("link", {name:"My collection"}).click();
  await expect(page.locator(".shop-card")).toHaveCount(3);
  await page.getByRole("button", { name: "Use Classic on board" }).click();
  await expect(page.getByRole("button", { name: "Use player colors" })).toBeVisible();

  const second = await browser.newContext({baseURL});
  try {
    expect((await second.request.post("/auth/login/begin", { data: {username} })).ok()).toBe(true);
    const other = await second.newPage();
    await other.goto("/shop?tab=collection");
    await expect(other.getByRole("link", { name: "Cobalt Enamel Equipped" })).toBeVisible();
    await expect(other.getByRole("button", { name: "Use player colors" })).toBeVisible();
    await other.goto("/shop?item=obsidian");
    await expect(other.locator(".unlock-rule")).toContainText("Earn 100 more");
    const denied = await second.request.post("/shop", { form: { action:"buy", item:"obsidian" } });
    expect(denied.status()).toBe(422);
    expect(await denied.text()).toContain("You need more coins");
    await other.goto("/shop?item=bingo");
    await expect(other.locator(".unlock-rule")).toContainText("Play all 7 tiles");
    await other.locator(".primary-menu").getByRole("link", { name: "Games", exact: true }).click();
    await expect(other).toHaveURL(new URL("/", baseURL).href);
  } finally {
    await second.close();
  }
});
