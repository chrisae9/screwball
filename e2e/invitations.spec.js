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
    await expect(inbox.getByRole("heading",{name:"Your games"})).toBeVisible();
    await expect(inbox.getByRole("button",{name:"Accept invitation"})).toHaveCount(0);
  } finally { await guest.close(); }
});

test("Chrome install steps match the phone and disappear in standalone mode", async ({browser}) => {
  const generic = "Mozilla/5.0 ";
  const examples = [
    [generic + "(iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 CriOS/141.0 Mobile/15E148 Safari/604.1", "Tap Share"],
    [generic + "(Linux; Android 15; Pixel 8) AppleWebKit/537.36 Chrome/141.0 Mobile Safari/537.36", "three-dot menu"],
  ];
  await Promise.all(examples.map(async ([userAgent, instruction]) => {
    const context = await browser.newContext({baseURL:"http://localhost:8123",userAgent,viewport:{width:390,height:844}});
    try {
      const page = await context.newPage();
      await page.request.post("/auth/login/begin",{data:{username:"scott"}});
      await page.goto("/");
      await page.getByText("Install Scrabble on your phone").click();
      await expect(page.locator("#install-guide ol")).toContainText(instruction);
      await context.addInitScript(() => Object.defineProperty(navigator,"standalone",{value:true}));
      await page.reload();
      await expect(page.locator("#install-guide")).toBeHidden();
    } finally {await context.close();}
  }));
});
