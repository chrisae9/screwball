import { expect, test } from "@playwright/test";
import { randomUUID } from "node:crypto";

test("account icon survives login and appears in another player's new and existing invite forms", async ({ page, browser, baseURL }) => {
  const host = await browser.newContext({baseURL});
  const player = "Icon " + randomUUID().slice(0, 8);
  try {
    await page.request.post("/auth/register/begin", {data:{username:player, display_name:player}});
    await host.request.post("/auth/login/begin", {data:{username:"shelli"}});
    const [legacy] = await (await page.request.get("/api/players?q=Shelli")).json();
    expect(legacy.avatar_id).toBe(Number(BigInt("0x" + legacy.id.replaceAll("-", "")) % 50n));
    expect(Object.keys(legacy).toSorted()).toEqual(["avatar_id","id","name"]);

    await page.goto("/settings");
    const chooser = page.locator(".avatar-picker");
    const summary = chooser.locator("summary");
    const savedIcon = summary.locator(".player-avatar");
    const initial = Number(await savedIcon.getAttribute("data-avatar-id"));
    const catalog = await (await page.request.get("/public/player-icons.json")).json();
    expect(catalog).toHaveLength(50);
    const choice = catalog[initial === 49 ? 48 : 49];
    await expect(chooser.getByRole("radio", {name:choice.name})).toBeHidden();
    await summary.click();
    await expect(chooser.getByRole("radio")).toHaveCount(50);
    const radio = chooser.getByRole("radio", {name:choice.name, exact:true});
    await radio.check();
    await expect(radio).toBeChecked();
    await expect(chooser.locator("output")).toHaveText(choice.name);
    await chooser.getByRole("button", {name:"Save player icon"}).click();
    await expect(savedIcon).toHaveAttribute("data-avatar-id", String(choice.id));
    await expect(summary).toContainText(choice.name);
    expect((await page.request.post("/settings/avatar", {form:{avatar_id:"50"}, maxRedirects:0})).status()).toBe(400);

    await page.request.post("/auth/logout", {maxRedirects:0});
    await page.request.post("/auth/login/begin", {data:{username:player}});
    await page.goto("/settings");
    await expect(savedIcon).toHaveAttribute("data-avatar-id", String(choice.id));
    const other = await host.newPage();
    await other.goto("/settings");
    await expect(other.locator(".avatar-picker summary .player-avatar")).toHaveAttribute("data-avatar-id",String(legacy.avatar_id));

    const assertInvite = async path => {
      await other.goto(path);
      await other.getByRole("searchbox").fill(player);
      const button = other.getByRole("button", {name:player, exact:true});
      await expect(button.locator(".player-avatar")).toHaveAttribute("data-avatar-id", String(choice.id));
      await button.click();
      const selected = other.locator(".player-picker:not([hidden]) summary").first();
      await expect(selected).toContainText(player);
      await expect(selected.locator(".player-avatar")).toHaveAttribute("data-avatar-id",String(choice.id));
    };
    await assertInvite("/games/new");
    await assertInvite("/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6/invite");
  } finally {
    await host.close();
  }
});
