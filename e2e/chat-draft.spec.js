import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";

async function register(client) {
  const username = "Draft player " + randomUUID().slice(0, 8);
  expect((await client.post("/auth/register/begin", {data: {username}})).ok()).toBe(true);
  return username;
}

async function createGame(client, guest) {
  const matches = await (await client.get("/api/players?q=" + encodeURIComponent(guest))).json();
  const recipient = matches.find(player => player.name === guest);
  expect(recipient).toBeDefined();
  const response = await client.post("/games", {form: {seat2: "invite:" + recipient.id}, maxRedirects: 0});
  expect(response.status()).toBe(303);
  return response.headers().location;
}

async function reopenFromBoard(page, path) {
  await page.getByRole("link", {name: "Back to board"}).click();
  await expect(page).toHaveURL(url => url.pathname === path);
  await page.getByRole("link", {name: /^Chat/}).click();
  await expect(page.getByRole("heading", {name: "Game chat"})).toBeVisible();
}

test("a chat draft follows its player and game through board navigation", async ({page, browser, baseURL}) => {
  const other = await browser.newContext({baseURL});
  try {
    const [writer, guest] = await Promise.all([register(page.request), register(other.request)]);
    const [game, secondGame] = await Promise.all([createGame(page.request, guest), createGame(page.request, guest)]);
    expect((await other.request.post(game + "/join", {form: {}, maxRedirects: 0})).status()).toBe(303);
    const input = page.getByRole("textbox", {name: "Message"});
    await page.goto(game + "/chat");
    await expect(page.locator(".chat-intro")).toContainText("Start the conversation");
    const text = "Trying a word here…\n  one more tile ";
    await input.fill(text);
    await reopenFromBoard(page, game);
    await expect(input).toHaveValue(text);
    await page.goto(secondGame + "/chat");
    await expect(input).toBeEmpty();
    await input.fill("Different match");
    expect((await page.request.post("/auth/login/begin", {data: {username: guest}})).ok()).toBe(true);
    await page.goto(game + "/chat");
    await expect(input).toBeEmpty();
    await input.fill("A separate player’s note");
    expect((await page.request.post("/auth/login/begin", {data: {username: writer}})).ok()).toBe(true);
    await page.goto(game + "/chat");
    await expect(input).toHaveValue(text);
    await input.fill("");
    await reopenFromBoard(page, game);
    await expect(input).toBeEmpty();
    await page.goto(secondGame + "/chat");
    await expect(input).toHaveValue("Different match");
  } finally {
    await other.close();
  }
});

test("a lost send acknowledgement can be retried after leaving without posting twice", async ({page, request}) => {
  const [, guest] = await Promise.all([register(page.request), register(request)]);
  const game = await createGame(page.request, guest);
  const endpoint = "/api" + game + "/chat";
  const ids = [];
  await page.route("**" + endpoint, async route => {
    if (route.request().method() !== "POST") return route.continue();
    ids.push(route.request().postDataJSON().id);
    if (ids.length > 1) return route.continue();
    const delivered = await route.fetch();
    expect(delivered.ok()).toBe(true);
    await route.abort("failed");
  });
  await page.goto(game + "/chat");
  const input = page.getByRole("textbox", {name: "Message"});
  const send = page.getByRole("button", {name: "Send"});
  const text = "I found a place for that tile.";
  await expect(page.locator(".chat-intro")).toContainText("Start the conversation");
  await input.fill(text);
  await send.click();
  await expect(page.locator(".chat-status")).toContainText("confirm delivery");
  await reopenFromBoard(page, game);
  await expect(input).toHaveValue(text);
  await expect(page.locator(".chat-status")).toContainText("confirm delivery");
  await send.click();
  await expect(input).toBeEmpty();
  expect(ids).toHaveLength(2);
  expect(ids[1]).toBe(ids[0]);
  const history = await (await page.request.get(endpoint)).json();
  expect(history.messages.map(message => message.text)).toEqual([text]);
  await reopenFromBoard(page, game);
  await expect(input).toBeEmpty();
  await expect(page.locator(".chat-status")).toBeEmpty();
});

test("a delayed acknowledgement preserves a new message being composed", async ({page, request}) => {
  const [, guest] = await Promise.all([register(page.request), register(request)]);
  const game = await createGame(page.request, guest);
  const endpoint = "/api" + game + "/chat";
  let release;
  const hold = new Promise(resolve => { release = resolve; });
  await page.route("**" + endpoint, async route => {
    if (route.request().method() !== "POST") return route.continue();
    const delivered = await route.fetch();
    await hold;
    await route.fulfill({response: delivered});
  });
  await page.goto(game + "/chat");
  await expect(page.locator(".chat-intro")).toContainText("Start the conversation");
  const input = page.getByRole("textbox", {name: "Message"});
  const send = page.getByRole("button", {name: "Send"});
  await input.fill("First message");
  try {
    await send.click();
    await expect(send).toBeDisabled();
    await input.fill("Next idea, still writing");
  } finally {
    release();
  }
  await expect(send).toBeEnabled();
  await reopenFromBoard(page, game);
  await expect(input).toHaveValue("Next idea, still writing");
  await expect(page.locator(".chat-status")).toBeEmpty();
  await expect(page.locator(".chat-bubble")).toHaveText(["First message"]);
});
