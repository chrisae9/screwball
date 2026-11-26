import {expect, test} from "@playwright/test";
import {randomUUID} from "node:crypto";

test("a player can rename an existing match identity, keep signing in, and remove the public username", async ({page, browser, baseURL}) => {
  const friend = await browser.newContext({baseURL});
  const suffix = randomUUID().slice(0, 8);
  const login = "login-" + suffix;
  const displayName = "Alex " + suffix;
  const handle = "tile_" + suffix;
  const renamed = "word_" + suffix;
  try {
    await page.request.post("/auth/register/begin", {data:{username:login, display_name:displayName}});
    await friend.request.post("/auth/register/begin", {data:{username:"friend-" + suffix, display_name:"Friend " + suffix}});
    const [legacy] = await (await friend.request.get("/api/players?q=" + suffix)).json();
    expect(legacy.name).toBe(displayName);
    expect(legacy.handle).toBeUndefined();
    const create = await friend.request.post("/games", {form:{seat2:"invite:" + legacy.id}, maxRedirects:0});
    const game = create.headers().location;
    expect((await page.request.post(game + "/join", {maxRedirects:0})).status()).toBe(303);
    const state = async () => (await page.request.get(game + "/state")).json();
    const original = await state();
    expect(original.seats[original.your_seat].name).toBe(displayName);

    await page.goto("/settings");
    const profile = page.locator("#player-username");
    await profile.locator("summary").click();
    await profile.getByRole("textbox", {name:"Player username"}).fill(" @" + handle.toUpperCase() + " ");
    await profile.getByRole("button", {name:"Save username"}).click();
    await expect(profile.locator("summary")).toContainText("@" + handle);
    await expect(page.getByRole("status").filter({hasText:"Scrabble username saved."})).toBeVisible();

    const other = await friend.newPage();
    await other.goto("/games/new");
    await other.getByRole("searchbox").fill("@" + handle.toUpperCase());
    const match = other.locator(".picker-choice").filter({hasText:"@" + handle});
    await expect(match).toContainText(displayName);
    await match.click();
    await expect(other.locator(".player-picker:not([hidden]) summary").first()).toContainText("@" + handle);

    await other.goto("/settings");
    await other.locator("#player-username summary").click();
    await other.getByRole("textbox", {name:"Player username"}).fill(handle.toUpperCase());
    await other.getByRole("button", {name:"Save username"}).click();
    await expect(other.getByRole("alert")).toContainText("taken");
    await expect(other.getByRole("textbox", {name:"Player username"})).toHaveValue(handle.toUpperCase());
    const foreign = await friend.request.post("/settings/username", {form:{handle:"mine_" + suffix,user_id:legacy.id},maxRedirects:0});
    expect(foreign.status()).toBe(303);
    expect((await state()).seats[original.your_seat].name).toBe("@" + handle);
    const change = await page.request.post("/settings/username", {form:{handle:renamed}, maxRedirects:0});
    expect(change.status()).toBe(303);
    await page.request.post("/auth/logout", {maxRedirects:0});
    expect((await page.request.post("/auth/login/begin", {data:{username:login}})).status()).toBe(200);
    const current = await state();
    expect(current.your_seat).toBe(original.your_seat);
    expect(current.seats[current.your_seat].name).toBe("@" + renamed);
    expect((await (await friend.request.get("/api/players?q=" + handle)).json())).toEqual([]);
    await page.goto("/settings");
    await profile.locator("summary").click();
    await profile.getByRole("button", {name:"Remove username"}).click();
    await expect(profile.locator("summary")).toContainText("Choose a username");
    expect((await state()).seats[current.your_seat].name).toBe(displayName);
  } finally {
    await friend.close();
  }
});
