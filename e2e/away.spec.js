import { expect, test } from "@playwright/test";
import { randomUUID } from "node:crypto";

test("Away mode silences alerts across sessions while invitations remain playable", async ({page, browser, baseURL}) => {
  test.setTimeout(60000); // Several independent sessions reconcile on the five-second poll.
  const second = await browser.newContext({baseURL});
  const host = await browser.newContext({baseURL});
  try {
    const suffix = randomUUID().slice(0, 8);
    const player = "Away " + suffix;
    await page.request.post("/auth/register/begin", {data:{username:player}});
    await second.request.post("/auth/login/begin", {data:{username:player}});
    await host.request.post("/auth/register/begin", {data:{username:"Invite " + suffix}});
    const secondSettings = await second.newPage();
    await secondSettings.goto("/settings");
    await page.goto("/settings");
    await page.getByLabel("Auto-zoom").check();
    await page.getByRole("button", {name:"Save", exact:true}).click();
    const choices = page.getByLabel("Pause for");
    await expect(choices.locator("option")).toHaveText([
      "Indefinite", "1 hour", "4 hours", "8 hours", "1 day", "2 days", "1 week",
    ]);
    await expect(choices).toHaveValue("indefinite");
    await page.getByRole("button", {name:"Pause notifications"}).click();
    await expect(page.locator(".away-settings")).toHaveAttribute("data-away-state", "indefinite");
    await secondSettings.bringToFront();
    await expect(secondSettings.locator(".away-settings")).toHaveAttribute("data-away-state", "indefinite", {timeout:12000});
    expect((await second.request.get("/api/invitations")).headers()["x-scrabble-away-state"]).toBe("indefinite");
    const silent = await page.request.post("/api/push/test");
    expect(silent.status()).toBe(409);

    await page.getByLabel("Change pause to").selectOption("168");
    await page.getByRole("button", {name:"Update pause"}).click();
    const week = await page.locator("#away-until").getAttribute("datetime");
    expect(Math.abs(Date.parse(week) - Date.now() - 7 * 86400000)).toBeLessThan(60000);
    await secondSettings.bringToFront();
    await expect(secondSettings.locator("#away-until")).toHaveAttribute("datetime", week, {timeout:12000});
    await page.getByLabel("Change pause to").selectOption("8");
    await page.getByRole("button", {name:"Update pause"}).click();
    await expect(page.getByRole("button", {name:"Resume now"})).toBeVisible();
    await secondSettings.bringToFront();
    await expect(secondSettings.getByRole("button", {name:"Resume now"})).toBeVisible({timeout:12000});
    const until = Date.parse(await page.locator("#away-until").getAttribute("datetime"));
    expect(Math.abs(until - Date.now() - 8 * 3600000)).toBeLessThan(60000);
    await expect(secondSettings.locator("#away-until")).toHaveAttribute("datetime", await page.locator("#away-until").getAttribute("datetime"), {timeout:12000});
    await page.getByLabel("Change pause to").selectOption("indefinite");
    await page.getByRole("button", {name:"Update pause"}).click();
    await secondSettings.bringToFront();
    await expect(secondSettings.locator(".away-settings")).toHaveAttribute("data-away-state", "indefinite", {timeout:12000});
    expect((await second.request.get("/api/invitations")).headers()["x-scrabble-away"]).toBe("true");
    expect((await host.request.get("/api/invitations")).headers()["x-scrabble-away"]).toBe("false");
    expect((await second.request.post("/settings/away", {form:{hours:"99"}})).status()).toBe(400);
    const [recipient] = await (await host.request.get("/api/players?q=" + encodeURIComponent(player))).json();
    const created = await host.request.post("/games", {form:{seat2:"invite:" + recipient.id}, maxRedirects:0});
    expect(created.status()).toBe(303);
    await page.reload();
    await expect(page.getByRole("checkbox", {name:"Auto-zoom"})).toBeChecked();
    await expect(page.getByRole("link", {name:"View game invitations"})).toHaveCount(0);
    const inbox = await second.newPage();
    await inbox.goto("/");
    await expect(inbox.getByRole("button", {name:"Accept invitation"})).toBeVisible();
    await expect(inbox.getByRole("link", {name:"View game invitations"})).toHaveCount(0);
    await expect(inbox.locator("#phone-alerts, #notif-status")).toHaveCount(0);
    await page.getByRole("button", {name:"Resume now"}).click();
    await expect(page.getByRole("link", {name:"View game invitations"})).toBeVisible();
    await secondSettings.bringToFront();
    await expect(secondSettings.getByRole("button", {name:"Pause notifications"})).toBeVisible({timeout:12000});
    expect((await second.request.get("/api/invitations")).headers()["x-scrabble-away"]).toBe("false");
  } finally { await Promise.all([host.close(), second.close()]); }
});
