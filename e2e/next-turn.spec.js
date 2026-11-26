import {randomUUID} from "node:crypto";
import {expect, test} from "@playwright/test";

async function privateGame(host, guest, guestId) {
  const created = await host.post("/games", {form:{seat2:"invite:" + guestId}, maxRedirects:0});
  expect(created.status()).toBe(303);
  const board = created.headers().location;
  expect((await guest.post(board + "/join", {form:{}, maxRedirects:0})).status()).toBe(303);
  return board;
}

async function pass(client, board) {
  expect((await client.post(board + "/move", {data:{kind:"pass"}})).status()).toBe(200);
}

function action(page, name) {
  const controls = page.locator(".rack-area .controls");
  return controls.getByRole("button", {name, exact:true}).or(controls.getByRole("link", {name, exact:true}));
}

for (const status of ["waiting", "finished"]) {
  test(`a ${status} board sends a caught-up player home and follows their next playable turn live`, async ({page, request}) => {
    const host = page.request;
    const suffix = randomUUID().slice(0, 8);
    const guestName = "Turn guest " + suffix;
    const accounts = await Promise.all([
      host.post("/auth/register/begin", {data:{username:"Turn host " + suffix}}),
      request.post("/auth/register/begin", {data:{username:guestName}}),
    ]);
    expect(accounts.every(response => response.ok())).toBe(true);
    const [guest] = await (await host.get("/api/players?q=" + encodeURIComponent(guestName))).json();
    const [current, other] = await Promise.all([privateGame(host, request, guest.id), privateGame(host, request, guest.id)]);
    await pass(host, other);
    if (status === "finished") {
      expect((await host.post(current + "/abandon")).status()).toBe(200);
    } else {
      await pass(host, current);
    }

    const home = action(page, "Home");
    const next = action(page, "Next turn");
    await page.goto(current);
    await expect(home).toBeVisible();
    await home.click();
    await expect(page).toHaveURL(url => url.pathname === "/");

    await page.goto(current);
    await expect(home).toBeVisible();
    await pass(request, other);
    await expect(next).toBeVisible({timeout:10000});
    let target = other;
    if (status === "finished") {
      // The only available match disappears while the player is on this board.
      expect((await request.post(other + "/abandon")).status()).toBe(200);
      await expect(home).toBeVisible({timeout:10000});
      target = await privateGame(host, request, guest.id);
    } else {
      // Model the same player taking their other turn on a second device.
      await pass(host, other);
      await expect(home).toBeVisible({timeout:10000});
      await pass(request, other);
    }
    await expect(next).toBeVisible({timeout:10000});
    await next.click();
    await expect(page).toHaveURL(url => url.pathname === target);
    await expect(action(page, "Pass")).toBeEnabled();

    // Consume the last target after the tap but before its fresh lookup reaches the server.
    await page.goto(current);
    await expect(next).toBeVisible({timeout:10000});
    await page.route("**/api/my-games", async route => {
      await pass(host, target);
      await route.continue();
    }, {times:1});
    await next.click();
    await expect(page).toHaveURL(url => url.pathname === "/");
  });
}
