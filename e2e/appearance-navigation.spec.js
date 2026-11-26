import { test, expect } from "@playwright/test";

const game = "/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6";
const colors = {dark: "rgb(17, 19, 15)", light: "rgb(244, 241, 234)"};
const palette = (page, mode) => expect(page.locator("body")).toHaveCSS("background-color", colors[mode]);

test("appearance follows the phone or a saved override without disturbing a move", async ({page, context}) => {
  await page.request.post("/auth/login/begin", {data:{username:"shelli"}});
  await page.emulateMedia({colorScheme:"dark"});
  await page.goto("/settings");
  await expect(page.getByRole("radio", {name:"System"})).toBeChecked();
  await palette(page, "dark");
  await page.getByRole("radio", {name:"Light"}).check();
  await palette(page, "light");
  const board = await context.newPage();
  await board.emulateMedia({colorScheme:"dark"});
  await board.goto(game);
  await palette(board, "light");
  await board.locator(".board:visible .cell").nth(112).click();
  await board.locator(".rack .rack-tile:not(.tile-blank)").first().click();
  await expect(board.locator(".pending-cell")).toHaveCount(1);
  await page.getByRole("radio", {name:"Dark"}).check();
  await palette(board, "dark");
  await page.emulateMedia({colorScheme:"light"});
  await page.reload();
  await expect(page.getByRole("radio", {name:"Dark"})).toBeChecked();
  await palette(page, "dark");
  await page.getByRole("radio", {name:"System"}).check();
  await palette(page, "light");
  await board.emulateMedia({colorScheme:"light"});
  await palette(board, "light");
  await expect(board.locator(".pending-cell")).toHaveCount(1);
  await board.emulateMedia({colorScheme:"dark"});
  await palette(board, "dark");
});

test("diagnostics live in Settings and each screen has a single route back", async ({page}) => {
  await page.request.post("/auth/login/begin", {data:{username:"shelli"}});
  await page.goto("/");
  await expect(page.locator('a[href^="/debug/"]')).toHaveCount(0);
  await page.getByRole("link", {name:"Settings"}).click();
  await expect(page.locator('a[href="/"]')).toHaveCount(1);
  async function checkScreen(label) {
    await page.getByText("Diagnostics", {exact:true}).click();
    const target = page.getByRole("link", {name:label});
    expect((await target.boundingBox()).height).toBeGreaterThanOrEqual(44);
    await target.click();
    await page.getByRole("link", {name:"Back to settings"}).click();
    await expect(page.getByRole("heading", {name:"Settings"})).toBeVisible();
  }
  await checkScreen("Notification diagnostics");
  await checkScreen("Touch diagnostics");
  await page.goto(game + "/invite");
  await expect(page.locator('a[href="' + game + '"]')).toHaveCount(1);
  await page.getByRole("link", {name:"Back to board"}).click();
  await expect(page.getByRole("link", {name:"Back to games"})).toBeVisible();
});
