import {test, expect} from "@playwright/test";

test.use({viewport:{width:320,height:568},isMobile:true,hasTouch:true});

test("a touch swipe reaches the end of a long game history with a zoomed draft", async ({page, browserName}) => {
  test.skip(browserName !== "chromium", "This test uses Chromium touch events for a native scroll.");
  const board="/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6";
  await page.request.post("/auth/login/begin",{data:{username:"shelli"}});
  const game=await (await page.request.get(board+"/state")).json();
  game.moves=Array.from({length:36},(_,i)=>({
    seat:i%2,kind:"play",words:["AT"],points:2,delta:0,
  }));
  await page.route("**"+board+"/state",route=>route.fulfill({json:game}));
  await page.route("**"+board+"/events",route=>route.abort());
  // Share the fixture across test workers without editing its persisted game.
  await page.route("**"+board, async route=>{
    const response=await route.fetch();
    const html=(await response.text()).replace(/(id="game-state" type="application\/json">)[^<]+/, "$1"+JSON.stringify(game))
      .replace(/data-auto-zoom="[^"]*"/,'data-auto-zoom="true"');
    await route.fulfill({response,body:html});
  });
  await page.goto(board);
  const rack=page.locator(".rack button[data-tile-id]");
  await expect(rack.first()).toBeVisible();
  await page.locator('.board:visible .cell[data-row="7"][data-col="7"]').tap();
  await rack.filter({has:page.getByText("F",{exact:true})}).tap();
  await expect(page.locator(".pending-cell")).toHaveCount(1);
  await expect.poll(()=>page.locator("#game-island .board").evaluate(el=>el.style.transform)).toContain("scale(");
  await page.getByRole("button",{name:"Show all 36 moves"}).evaluate(button=>button.click());
  await expect(page.locator("#game-island .move-log li")).toHaveCount(36);
  const cdp=await page.context().newCDPSession(page);
  async function swipe() {
    await cdp.send("Input.dispatchTouchEvent",{type:"touchStart",touchPoints:[{x:160,y:535}]});
    await cdp.send("Input.dispatchTouchEvent",{type:"touchMove",touchPoints:[{x:160,y:375}]});
    await cdp.send("Input.dispatchTouchEvent",{type:"touchMove",touchPoints:[{x:160,y:165}]});
    await cdp.send("Input.dispatchTouchEvent",{type:"touchEnd",touchPoints:[]});
  }
  const abandon=page.getByRole("button",{name:"Abandon game"});
  await swipe();
  await expect.poll(()=>page.evaluate(()=>scrollY)).toBeGreaterThan(0);
  await swipe();
  await swipe();
  await swipe();
  await expect(abandon).toBeInViewport({ratio:1});
  const bottom=await abandon.evaluate(el=>el.getBoundingClientRect().bottom);
  expect(bottom).toBeLessThanOrEqual((await page.evaluate(()=>visualViewport.height)) - 12);
  await expect(page.getByText("Your other games",{exact:true})).toHaveCount(0);
});
