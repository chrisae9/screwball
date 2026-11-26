import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";

test("the private chat dot clears across devices only after incoming messages are visible", async ({page, browser, baseURL}, testInfo) => {
  test.setTimeout(90000);
  const guest = await browser.newContext({baseURL});
  const device = await browser.newContext({baseURL});
  const hostName = "Chat reader " + randomUUID().slice(0,8);
  const guestName = "Chat sender " + randomUUID().slice(0,8);
  try {
    await Promise.all([[page,hostName],[guest,guestName]].map(async ([client,username]) => {
      expect((await client.request.post("/auth/register/begin", {data:{username}})).ok()).toBe(true);
    }));
    const [player] = await (await page.request.get("/api/players?q="+encodeURIComponent(guestName))).json();
    const created = await page.request.post("/games",{form:{seat2:"invite:"+player.id}, maxRedirects:0});
    const board = created.headers().location;
    expect((await guest.request.post(board+"/join",{form:{},maxRedirects:0})).status()).toBe(303);
    const key = board.split("/").pop();
    const unread = async client => (await client.request.get("/api/invitations",{
      headers:{"x-scrabble-current-game":key},
    })).headers()["x-scrabble-chat-unread"];
    const send = async (client,text) => expect((await client.request.post("/api"+board+"/chat", {
      data:{id:randomUUID(),text},
    })).ok()).toBe(true);
    await page.goto(board);
    const icon = page.locator(".game-chat-link");
    await expect(icon).toHaveAccessibleName("Chat");
    const bounds = await icon.boundingBox();
    await send(guest, "Your rack looks ready!");
    await expect(icon).toHaveAccessibleName("Chat, unread messages",{timeout:12000});
    expect(await icon.boundingBox()).toEqual(bounds);
    expect(await icon.evaluate(el => getComputedStyle(el,"::after").width)).toBe("8px");
    await testInfo.attach("unread-chat-dot", {body:await page.screenshot(), contentType:"image/png"});
    expect(await unread(guest)).toBe("false");

    await device.request.post("/auth/login/begin",{data:{username:hostName}});
    const reader = await device.newPage();
    await reader.addInitScript(() => {
      Object.defineProperty(document,"hidden",{get:()=>window.testChatHidden, configurable:true});
      window.testChatHidden = true;
    });
    await reader.goto(board+"/chat");
    expect(await unread(device)).toBe("true");
    await reader.evaluate(()=>{window.testChatHidden=false;document.dispatchEvent(new Event("visibilitychange"));});
    await expect(reader.getByText("Your rack looks ready!")).toBeVisible();
    await expect.poll(()=>unread(device)).toBe("false");
    await expect(icon).toHaveAccessibleName("Chat",{timeout:12000});
    const receipt = reader.waitForResponse(response => response.url().endsWith("/chat/read")
      && response.status() === 204 && response.request().postDataJSON().seen.includes(2), {timeout:15000});
    await send(guest, "Still here?");
    await expect(reader.getByText("Still here?")).toBeVisible({timeout:10000});
    await receipt;
    expect(await unread(device)).toBe("false");

    // The latest page contains 50 rows. Visiting it must leave the earlier
    // unread page and offscreen rows unread until the reader scrolls to them.
    await reader.close();
    await Promise.all(Array.from({length:52}, (_, i) => send(guest, "Message " + i)));
    const paged = await device.newPage();
    const seen = new Set();
    paged.on("request", request => {
      if (request.url().endsWith("/chat/read")) {
        for (const seq of request.postDataJSON().seen) seen.add(seq);
      }
    });
    await paged.goto(board+"/chat");
    await expect(paged.locator(".chat-messages li")).toHaveCount(50,{timeout:15000});
    await expect.poll(()=>seen.has(54),{timeout:12000}).toBe(true);
    expect(seen.has(3)).toBe(false);
    expect(seen.has(5)).toBe(false);
    expect(await unread(device)).toBe("true");
    await paged.getByRole("button",{name:"Earlier messages"}).click();
    await expect(paged.locator(".chat-messages li")).toHaveCount(54);
    await paged.locator('li[data-seq="3"]').scrollIntoViewIfNeeded();
    await expect.poll(()=>seen.has(3),{timeout:12000}).toBe(true);
  } finally {
    await Promise.all([guest.close(), device.close()]);
  }
});
