import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";

for (const outcome of ["success", "error"]) {
  test(`a delayed move ${outcome} does not overwrite a turn recovered after reconnect`, async ({page, browser, baseURL}) => {
    const opponent = await browser.newContext({baseURL});
    const suffix = randomUUID().slice(0, 8);
    const host = "Host " + suffix;
    const guest = "Guest " + suffix;
    let release;
    const hold = new Promise(resolve => { release = resolve; });
    try {
      await Promise.all([[page,host], [opponent,guest]].map(async ([client,username]) => {
        expect((await client.request.post("/auth/register/begin", {data:{username}})).ok()).toBe(true);
      }));
      const [recipient] = await (await page.request.get("/api/players?q=" + encodeURIComponent(guest))).json();
      const created = await page.request.post("/games", {form:{seat2:"invite:" + recipient.id}, maxRedirects:0});
      expect(created.status()).toBe(303);
      const board = created.headers().location;
      expect((await opponent.request.post(board + "/join",{form:{},maxRedirects:0})).status()).toBe(303);
      await page.route("**" + board + "/events", route => route.abort());
      let committed;
      const firstMove = new Promise(resolve => { committed = resolve; });
      await page.route("**" + board + "/move", async route => {
        const response = await route.fetch();
        committed(response.status());
        await hold;
        if (outcome === "success") await route.fulfill({response});
        else await route.fulfill({status:503,json:{error:"Delayed move error"}});
      });
      await page.goto(board);
      const pass = page.getByRole("button",{name:"Pass",exact:true});
      page.once("dialog", dialog => dialog.accept());
      await pass.click();
      expect(await firstMove).toBe(200);
      expect((await opponent.request.post(board + "/move", {data:{kind:"pass"}})).ok()).toBe(true);
      await page.evaluate(()=>window.dispatchEvent(new Event("online")));
      const moves = page.locator("#game-island .move-log li");
      await expect(moves).toHaveCount(2);
      await expect(pass).toBeDisabled(); // Both turns are visible while the first reply is in flight.
      release();
      await expect(pass).toBeEnabled();
      await expect(moves).toHaveCount(2);
      await expect(page.getByRole("alert")).toHaveCount(0);
    } finally {
      release();
      await opponent.close();
    }
  });
}
