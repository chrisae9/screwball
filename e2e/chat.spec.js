import { test, expect } from "@playwright/test";
import { randomUUID } from "node:crypto";

test("players chat in a separate screen and can resume an unfinished move", async ({page, browser}, testInfo) => {
  const opponent = await browser.newContext({baseURL: "http://localhost:8123"});
  try {
    const other = await opponent.newPage();
    const suffix = randomUUID().slice(0,8);
    await Promise.all([[page, "Will"], [other, "June"]].map(async ([p, display_name]) => {
      const response = await p.request.post("/auth/register/begin", {data:{username:display_name + suffix, display_name}});
      expect(response.ok()).toBe(true);
    }));
    const created = await page.request.post("/games", {form:{seat2:"open"}, maxRedirects:0});
    const board = created.headers().location;
    expect((await other.request.post(board + "/join", {form:{}, maxRedirects:0})).status()).toBe(303);
    await page.goto(board);
    await expect(page.locator(".rack .rack-tile").first()).toBeVisible();
    await page.locator(".board:visible .cell").nth(112).click();
    await page.locator(".rack .rack-tile:not(.tile-blank)").first().click();
    await expect(page.locator(".pending-cell")).toHaveCount(1);
    await expect(page.getByRole("textbox", {name:"Message"})).toHaveCount(0);
    await testInfo.attach("board-with-chat-action", {body:await page.screenshot(), contentType:"image/png"});
    await page.getByRole("link", {name:"Chat", exact:true}).click();
    await other.goto(board + "/chat");
    await expect(page.getByRole("heading", {name:"Game chat"})).toBeVisible();
    await expect(page.locator(".chat-intro")).toContainText("Start the conversation");
    await other.getByRole("textbox", {name:"Message"}).fill("Good luck!");
    await other.getByRole("button", {name:"Send", exact:true}).click();
    await expect(page.locator(".chat-bubble")).toHaveText(["Good luck!"], {timeout:10000});
    await page.getByRole("textbox", {name:"Message"}).fill("You too! <3");
    await page.getByRole("button", {name:"Send", exact:true}).click();
    await expect(other.locator(".chat-bubble")).toHaveText(["Good luck!", "You too! <3"], {timeout:10000});
    await page.reload();
    await expect(page.locator(".chat-bubble")).toHaveText(["Good luck!", "You too! <3"]);
    const layout = await page.locator(".chat-form").evaluate(el => ({
      width: document.documentElement.scrollWidth - innerWidth,
      bottom: Math.round(el.getBoundingClientRect().bottom - innerHeight),
    }));
    expect(layout.width).toBeLessThanOrEqual(1);
    expect(layout.bottom).toBeLessThanOrEqual(1);
    await testInfo.attach("game-chat", {body:await page.screenshot(), contentType:"image/png"});
    await page.getByRole("link", {name:"Back to board"}).click();
    await expect(page.locator(".pending-cell")).toHaveCount(1);
    await expect(page.getByRole("textbox", {name:"Message"})).toHaveCount(0);
    // A second tab may finish the turn while this phone is reading chat.
    await page.getByRole("link", {name:"Chat", exact:true}).click();
    expect((await page.request.post(board + "/move", {data:{kind:"pass"}})).ok()).toBe(true);
    await page.getByRole("link", {name:"Back to board"}).click();
    await expect(page.locator(".rack .rack-tile").first()).toBeVisible();
    await expect(page.locator(".pending-cell")).toHaveCount(0);
  } finally {
    await opponent.close();
  }
});

test("incoming invitations and reconnect notices leave the phone chat composer accessible", async ({ page }) => {
  await page.setViewportSize({width: 320, height: 568});
  await page.request.post("/auth/login/begin", {data: {username: "shelli"}});
  const path = "/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6/chat";
  let paused = false;
  await page.route("**/api/invitations", route => route.fulfill(paused
    ? {status: 503, body: "Temporarily unavailable"}
    : {contentType: "text/html", body: "<section>Game invitation</section>"}));
  await page.goto(path);
  const heading = page.getByRole("navigation");
  await expect(heading.getByRole("link", {name: "View game invitations"})).toBeVisible();
  paused = true;
  const failPoll = () => Promise.all([
      page.waitForResponse(response => response.url().endsWith("/api/invitations") && response.status() === 503),
      page.evaluate(() => window.dispatchEvent(new Event("focus"))),
  ]);
  await failPoll();
  await failPoll();
  await expect(heading.getByRole("button", {name: /Reconnect/})).toBeVisible();
  const send = page.getByRole("button", {name:"Send", exact:true});
  expect(await send.evaluate(button => {
    const rect = button.getBoundingClientRect();
    return rect.bottom <= innerHeight
      && document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2) === button;
  })).toBe(true);
});

test("an unread chat and a capped activity count fit below the phone status edge", async ({page}) => {
  await page.setViewportSize({width: 320, height: 693});
  await page.request.post("/auth/login/begin", {data:{username:"shelli"}});
  await page.route("**/api/invitations", route => route.fulfill({
    contentType:"text/html", body:"",
    headers:{"x-scrabble-other-turns":"12", "x-scrabble-chat-unread":"true"},
  }));
  await page.goto("/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6");
  const chat = page.getByRole("link", {name:"Chat, unread messages"});
  const bell = page.getByRole("link", {name:"View games waiting for your turn"});
  await expect(chat).toBeVisible();
  await expect(bell).toHaveAttribute("href", "/#my-games");
  const count = bell.locator(".notice-count");
  await expect(count).toHaveText("9+");
  const badge = await count.boundingBox();
  const target = await bell.boundingBox();
  // The installed iPhone app paints a six-pixel strip above a flush toolbar.
  expect(badge.y - target.y).toBeGreaterThanOrEqual(6);
  expect(badge.x + badge.width).toBeLessThanOrEqual(target.x + target.width);
  expect((await chat.boundingBox()).x + 44).toBeLessThanOrEqual(target.x);
});
