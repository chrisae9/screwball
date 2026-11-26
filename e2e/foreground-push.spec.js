import { expect, test } from "@playwright/test";
import { randomUUID } from "node:crypto";

test("foreground push leases follow each window and release on background or close", async ({ page, context, request }) => {
  const id = randomUUID();
  const endpoint = "https://push.example.test/" + id;
  const subscribe = await context.request.post("/auth/register/begin", {data: {username:"presence-" + id.slice(0, 8)}});
  expect(subscribe.ok()).toBe(true);
  const sub = {endpoint, keys:{p256dh:"fixture-key", auth:"fixture-auth"}};
  expect((await context.request.post("/api/push/subscribe", {data:sub})).ok()).toBe(true);

  const sample = {endpoint, window_id:randomUUID(), sequence:1, visible:true};
  expect((await request.post("/api/push/presence", {data:sample})).status()).toBe(401);
  await request.post("/auth/register/begin", {data:{username:"another-" + id.slice(0, 8)}});
  expect((await request.post("/api/push/presence", {data:sample})).status()).toBe(404);
  expect((await context.request.post("/api/push/presence", {
    data: {...sample, endpoint: endpoint + "-unregistered"},
  })).status()).toBe(404);

  await context.addInitScript(deviceEndpoint => {
    // Browser automation omits the body of native Beacon requests. Keep a
    // read-only copy for assertions and still let the native Beacon reach Rust.
    const nativeBeacon = navigator.sendBeacon.bind(navigator);
    navigator.sendBeacon = (url, blob) => {
      if (url === "/api/push/presence") window.lastPresenceBeacon = blob.text().then(JSON.parse);
      return nativeBeacon(url, blob);
    };
    Object.defineProperty(window, "Notification", {configurable:true, value:{permission:"granted"}});
    Object.defineProperty(navigator.serviceWorker, "getRegistration", {value:async () => ({
      pushManager:{getSubscription:async () => ({endpoint:deviceEndpoint})},
    })});
  }, endpoint);
  const presence = async (p, active, action) => {
    // A response to an earlier heartbeat can arrive after a visibility change.
    // Identify the newly sent request, then check its own acknowledgement.
    const report = p.waitForRequest(request => {
      if (!request.url().endsWith("/api/push/presence")) return false;
      const data = request.postDataJSON();
      return data ? data.endpoint === endpoint && data.visible === active : !active;
    }, {timeout: 12000});
    if (action) await action();
    const sent = await report;
    expect((await sent.response())?.status()).toBe(204);
    return sent.postDataJSON() ?? await p.evaluate(() => window.lastPresenceBeacon);
  };
  const first = await presence(page, true, () => page.goto("/leaderboard"));
  const next = await presence(page, true);
  expect(next.window_id).toBe(first.window_id);
  expect(next.sequence).toBeGreaterThan(first.sequence);
  const second = await context.newPage();
  const another = await presence(second, true, () => second.goto("/leaderboard"));
  expect(another.window_id).not.toBe(first.window_id);
  const hidden = await presence(second, false, () => second.evaluate(() => {
    Object.defineProperty(document, "visibilityState", {configurable:true, value:"hidden"});
    document.dispatchEvent(new Event("visibilitychange"));
  }));
  expect(hidden.window_id).toBe(another.window_id);
  const resumed = await presence(second, true, () => second.evaluate(() => {
    Object.defineProperty(document, "visibilityState", {configurable:true, value:"visible"});
    document.dispatchEvent(new Event("visibilitychange"));
  }));
  expect(resumed.sequence).toBeGreaterThan(hidden.sequence);
  const closing = await presence(second, false, () => second.evaluate(() => window.dispatchEvent(new Event("pagehide"))));
  expect(closing.window_id).toBe(another.window_id);
  expect(closing.sequence).toBeGreaterThan(resumed.sequence);
});

test("a delayed browser registration recovers after sign-in and repairs only its current subscription", async ({ page, context }) => {
  const id = randomUUID();
  const endpoint = "https://push.example.test/repair-" + id;
  await context.addInitScript(deviceEndpoint => {
    Object.defineProperty(window, "Notification", {configurable:true, value:{permission:"granted"}});
    const device = {
      endpoint: deviceEndpoint,
      toJSON: () => ({endpoint: deviceEndpoint, keys:{p256dh:"fixture-key", auth:"fixture-auth"}}),
    };
    Object.defineProperty(navigator.serviceWorker, "getRegistration", {value:async () => {
      window.registrationLookups = (window.registrationLookups || 0) + 1;
      return window.workerReady ? {pushManager:{getSubscription: async () => device}} : undefined;
    }});
  }, endpoint);
  const responses = [];
  page.on("response", response => {
    const path = new URL(response.url()).pathname;
    if (path === "/api/push/presence" || path === "/api/push/subscribe") {
      responses.push([path, response.status()]);
    }
  });
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => window.registrationLookups)).toBeGreaterThan(0);
  await page.evaluate(() => { window.workerReady = true; });
  await expect.poll(() => responses, {timeout:20000}).toContainEqual(["/api/push/presence",401]);
  expect(responses.filter(([path]) => path === "/api/push/subscribe")).toHaveLength(0);
  await context.request.post("/auth/register/begin", {data:{username:"repair-" + id.slice(0,8)}});
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => responses).toContainEqual(["/api/push/presence",204]);
  expect(responses).toContainEqual(["/api/push/presence",404]);
  expect(responses.filter(([path]) => path === "/api/push/subscribe")).toEqual([["/api/push/subscribe",200]]);
});
