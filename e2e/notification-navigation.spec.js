import {expect, test} from "@playwright/test";
import {randomUUID} from "node:crypto";

test("returning to Games never inserts an alerts panel or prompts during a slow notification setup", async ({page, context}) => {
  await context.addInitScript(() => {
    const storage = window.sessionStorage;
    const count = key => storage.setItem(key, String(Number(storage.getItem(key)) + 1));
    // Persist permission across full navigations just like the installed app.
    Object.defineProperty(window, "Notification", {value: {
      get permission() { return storage.getItem("permission") || "default"; },
      requestPermission() {
        count("permissionRequests");
        sessionStorage.setItem("permission", "granted");
        return Promise.resolve("granted");
      }
    }});
    Object.defineProperty(window, "PushManager", {value:{}});
    const worker = new EventTarget();
    worker.getRegistration = async () => undefined;
    worker.register = async () => { count("registrations"); };
    worker.ready = Promise.resolve({pushManager:{getSubscription:async () => {
      count("pendingSubscriptions");
      return new Promise(() => {}); // Device or network is still busy when the user leaves.
    }}});
    Object.defineProperty(navigator, "serviceWorker", {value:worker});
    Object.defineProperty(navigator, "standalone", {value:true});
    if (location.pathname === "/") {
      const watch = new MutationObserver(() => {
        if (document.querySelector("#phone-alerts, #notif-enable, #install-guide")) count("promptFrames");
      });
      watch.observe(document,{childList:true,subtree:true,attributes:true});
    }
  });
  const user = "AlertNavigation" + randomUUID().slice(0,8);
  await page.request.post("/auth/register/begin", {data:{username:user}});
  await page.route("**/api/push/vapid-public-key",r=>r.fulfill({json:{public_key:"fixture"}}));
  await page.goto("/");
  const gamesReady = async () => {
    await expect(page).toHaveURL(/\/$/);
    await expect(page.getByRole("heading",{name:"Games",exact:true})).toBeVisible();
    await expect(page.locator('#phone-alerts, #notif-enable, #install-guide, script[src*="/settings.js"]')).toHaveCount(0);
  };
  await gamesReady();
  await page.getByRole("link",{name:"Settings",exact:true}).click();
  const enable = page.getByRole("button",{name:"Enable notifications"});
  await expect(enable).toBeEnabled();
  await enable.dblclick();
  await expect.poll(() => page.evaluate(() => sessionStorage.getItem("pendingSubscriptions"))).toBe("1");
  await page.goBack();
  await gamesReady();
  // Drive a persisted restoration too, including engines that disable bfcache in automation.
  await page.evaluate(() => {
    window.dispatchEvent(new PageTransitionEvent("pageshow",{persisted:true}));
    window.dispatchEvent(new Event("focus"));
    document.dispatchEvent(new Event("visibilitychange"));
    window.dispatchEvent(new Event("online"));
  });
  await page.request.post("/settings/away",{form:{hours:"indefinite"},maxRedirects:0});
  const activity = page.waitForResponse(r=>r.url().endsWith("/api/activity"));
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await activity;
  await gamesReady();
  await page.getByRole("link",{name:"History",exact:true}).click();
  await page.goBack();
  await gamesReady();
  await page.getByRole("link",{name:"Leaderboard",exact:true}).click();
  await page.goBack();
  await gamesReady();
  const result = await page.evaluate(() => ["promptFrames","permissionRequests","registrations","pendingSubscriptions"]
    .map(key=> Number(sessionStorage.getItem(key))));
  expect(result).toEqual([0,1,1,1]);
});
