import { test, expect } from "@playwright/test";
import { randomUUID } from "node:crypto";

test("find a player, receive an invitation, and accept the reserved seat", async ({ browser, page }) => {
  const guest = await browser.newContext({ baseURL: "http://localhost:8123" });
  const inbox = await guest.newPage();
  const suffix = randomUUID().slice(0, 8);
  const sender = "Host " + suffix;
  const recipient = "Guest " + suffix;
  try {
    await Promise.all([[page, sender], [inbox, recipient]].map(async ([p, name]) => {
      const response = await p.request.post("/auth/register/begin", { data: { username: name, display_name: name } });
      expect(response.ok()).toBe(true);
    }));
    await inbox.goto("/");
    await page.goto("/games/new");
    const picker = page.getByRole("searchbox", {name:"Player for seat 2"});
    await picker.fill(recipient);
    await page.getByRole("button", { name:recipient, exact:true }).click();
    await page.getByRole("button", {name:"Create game"}).click();
    await expect(page.getByRole("link", {name:"Back to games"})).toBeVisible();
    await expect(inbox.getByText(sender + " invited you to play Scrabble.")).toBeVisible({timeout: 22000});
    await inbox.getByRole("button",{name:"Accept invitation"}).click();
    expect(new URL(inbox.url()).pathname).toBe(new URL(page.url()).pathname);
    await expect(inbox.locator(".rack .rack-tile")).toHaveCount(7);
    await inbox.getByRole("link",{name:"Back to games"}).click();
    await expect(inbox.getByRole("heading",{name:"Games", exact:true})).toBeVisible();
    await expect(inbox.getByRole("button",{name:"Accept invitation"})).toHaveCount(0);
  } finally { await guest.close(); }
});

test("Settings install steps match the phone and disappear in standalone mode", async ({browser, baseURL}) => {
  const generic = "Mozilla/5.0 ";
  const examples = [
    [generic + "(iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 CriOS/141.0 Mobile/15E148 Safari/604.1", "Tap Share"],
    [generic + "(iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Version/18.0 Mobile/15E148 Safari/604.1", "Share menu in Safari"],
    [generic + "(Linux; Android 15; Pixel 8) AppleWebKit/537.36 Chrome/141.0 Mobile Safari/537.36", "three-dot menu"],
  ];
  await Promise.all(examples.map(async ([userAgent, instruction]) => {
    const context = await browser.newContext({baseURL,userAgent,viewport:{width:390,height:844}});
    try {
      const page = await context.newPage();
      await page.request.post("/auth/login/begin",{data:{username:"scott"}});
      await page.goto("/settings");
      await page.getByText("Install Scrabble on your phone").click();
      await expect(page.locator("#install-guide ol")).toContainText(instruction);
      await context.addInitScript(() => Object.defineProperty(navigator,"standalone",{value:true}));
      await page.reload();
      await expect(page.locator("#install-guide")).toBeHidden();
    } finally {await context.close();}
  }));
});

test("a waiting host dashboard and recipient Settings update without navigation", async ({ browser, page }) => {
  const suffix = randomUUID().slice(0, 8);
  const recipient = await browser.newContext({baseURL:"http://localhost:8123"});
  try {
    const settings = await recipient.newPage();
    const host = page.context();
    const sender = "Sender " + suffix;
    const name = "Player " + suffix;
    await Promise.all([[host, sender], [recipient, name]].map(async ([context, username]) => {
      expect((await context.request.post("/auth/register/begin", {data:{username}})).ok()).toBe(true);
    }));
    await Promise.all([page.goto("/"), settings.goto("/settings")]);
    const directory = await (await host.request.get("/api/players?q=" + encodeURIComponent(name))).json();
    const created = await host.request.post("/games", {form:{seat2:"invite:" + directory[0].id}, maxRedirects:0});
    const board = created.headers().location;
    const row = page.locator('#my-games a[href="' + board + '"]');
    await expect(row).toContainText(name + " (invited)");
    await settings.getByRole("link", {name:"View game invitations"}).click();
    await settings.getByRole("button", {name:"Accept invitation"}).click();
    await expect(row).toContainText(name);
    await expect(row).not.toContainText("(invited)");
    const turn = page.locator('[data-group="turn"] a[href="' + board + '"]');
    await expect(turn).toBeVisible();
    expect((await host.request.post(board + "/move", {data:{kind:"pass"}})).ok()).toBe(true);
    await expect(turn).toHaveCount(0);
    expect((await recipient.request.post(board + "/move", {data:{kind:"pass"}})).ok()).toBe(true);
    await expect(turn).toBeVisible();
  } finally { await recipient.close(); }
});

test("phone returning online catches missed games even without an event stream", async ({page,context}) => {
  await page.route("**/api/activity/events", route => route.abort());
  const username = "Resume " + randomUUID().slice(0, 8);
  await context.request.post("/auth/register/begin",{data:{username}});
  await page.goto("/");
  await expect(page.locator(".lobby-empty")).toContainText("No games yet");
  // APIRequestContext keeps the server side of the test independent of the offline phone.
  await context.setOffline(true);
  const create = await context.request.post("/games",{form:{seat2:"easy"},maxRedirects:0});
  await context.setOffline(false);
  await expect(page.locator('#my-games a[href="' + create.headers().location + '"]')).toBeVisible();
});

test("granted device repairs server registration and offers a retry on failure", async ({page,context}) => {

  await page.addInitScript(() => {
    Object.defineProperty(Notification, "permission", {get:() => "granted"});
    const worker = new EventTarget();
    const registration = {pushManager:{getSubscription:async () => ({toJSON:() => ({endpoint:"test-device"})})}};
    worker.register = async () => registration;
    worker.ready = Promise.resolve(registration);
    Object.defineProperty(navigator, "serviceWorker", {value:worker});
    Object.defineProperty(navigator, "standalone", {value:true});
  });
  let attempts = 0;
  await page.route("**/api/push/vapid-public-key",r => r.fulfill({json:{public_key:"fixture"}}));
  await page.route("**/api/push/subscribe",r => {
    attempts++;
    return r.fulfill(attempts === 1 ? {status:503,body:"offline"} : {json:{ok:true}});
  });
  await context.request.post("/auth/login/begin",{data:{username:"scott"}});
  await page.goto("/settings");
  await expect(page.locator("#notif-status")).toContainText("Tap to retry");
  await page.getByRole("button",{name:"Enable notifications"}).click();
  await expect(page.locator("#notif-status")).toHaveText("Notifications are on.");
  expect(attempts).toBe(2);
});

test("the Games and History bell counts live activity, opens filtered destinations and respects Away", async ({page, browser, baseURL}) => {
  const host = await browser.newContext({baseURL});
  try {
    const guest = "Bell " + randomUUID().slice(0,8);
    const sender = "Inviter " + randomUUID().slice(0,8);
    await Promise.all([[page.request,guest],[host.request,sender]].map(([api,username])=>
      api.post("/auth/register/begin", {data:{username}})));
    const match = await page.request.post("/games", {form:{seat2:"easy"},maxRedirects:0});
    const mine = match.headers().location;
    await page.goto("/");
    const turnBell = page.getByRole("link",{name:"View games waiting for your turn"});
    await expect(turnBell).toHaveText("1");
    await page.getByRole("button",{name:"Open games",exact:true}).click();
    await page.getByRole("searchbox").fill("_unmatched_");
    await turnBell.click();
    await expect(page.getByRole("button",{name:"Your turn",exact:true})).toHaveAttribute("aria-pressed","true");
    await expect(page.getByRole("searchbox")).toBeEmpty();
    await expect(page.locator('#my-games a[href="'+mine+'"]')).toBeVisible();
    const [recipient] = await (await host.request.get("/api/players",{params:{q:guest}})).json();
    const sent = await host.request.post("/games",{form:{seat2:"invite:"+recipient.id},maxRedirects:0});
    expect(sent.status()).toBe(303);
    await page.goto("/history");
    const inviteBell = page.getByRole("link",{name:"View game invitations"});
    await expect(inviteBell).toHaveText("2");
    await inviteBell.click();
    await expect(page.getByRole("button",{name:"Accept invitation"})).toBeVisible();
    // The exact same hash must work again after the player switches filters.
    await page.getByRole("button",{name:"Open games",exact:true}).click();
    await page.getByRole("searchbox").fill("_absent_");
    await inviteBell.click();
    await expect(page.getByRole("searchbox")).toBeEmpty();
    await expect(page.getByRole("button",{name:"Accept invitation"})).toBeVisible();
    await page.request.post("/settings/away",{form:{hours:"indefinite"},maxRedirects:0});
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await expect(inviteBell).toHaveCount(0);
    await page.goto("/history");
    await expect(page.getByRole("heading",{name:"History",exact:true})).toBeVisible();
    await expect(inviteBell).toHaveCount(0);
  } finally { await host.close(); }
});
