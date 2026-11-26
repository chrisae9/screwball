import { test, expect } from "@playwright/test";

test.use({hasTouch:true, isMobile:true, viewport:{width:768,height:1024}});

test("tablet board, touch controls, standings and chat adapt to rotation and split view", async ({page}) => {
  const board = "/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6";
  await page.request.post("/auth/login/begin", {data:{username:"shelli"}});
  await page.goto(board);
  const tile = letter => page.locator(".rack button").filter({
    has:page.locator(".tile-letter").filter({hasText:new RegExp("^" + letter + "$")}),
  }).first();
  await expect(tile("F")).toBeVisible();
  await page.locator('.board:visible .cell[data-row="7"][data-col="7"]').tap();
  await tile("F").tap();
  await tile("A").tap();
  await tile("R").tap();
  const review = page.locator(".draft-review");
  await expect(review).toHaveAttribute("data-state","valid");

  async function checkSize(width, height) {
    await page.setViewportSize({width,height});
    await page.evaluate(() => scrollTo(0,0));
    const layout = await page.locator("#game-island").evaluate(island => {
      const get = selector => {
        const rect=island.querySelector(selector).getBoundingClientRect();
        return {x:rect.x,y:rect.y,width:rect.width,height:rect.height,right:rect.right,bottom:rect.bottom};
      };
      return {board:get(".board"),rack:get(".rack"),key:get(".rack button[data-tile-id]"),
        undo:get(".rack-backspace-tile"),play:get(".play-button"),side:get(".sidebar"),
        pageWidth:document.documentElement.scrollWidth};
    });
    expect(layout.board.width).toBeGreaterThanOrEqual(Math.min(width * .44, (height - 305)));
    expect(layout.board.bottom).toBeLessThanOrEqual(layout.rack.y + 1);
    expect(layout.key.height).toBeGreaterThanOrEqual(44);
    expect(layout.undo.height).toBeGreaterThanOrEqual(44);
    expect(layout.play.height).toBeGreaterThanOrEqual(44);
    expect(layout.play.bottom).toBeLessThanOrEqual(height + 1);
    expect(layout.pageWidth).toBeLessThanOrEqual(width + 1);
    if (width > height) expect(layout.side.x).toBeGreaterThanOrEqual(layout.board.right);
    else expect(layout.side.y).toBeGreaterThanOrEqual(layout.play.bottom);
    const scores=page.locator("#game-island .scoreboard");
    await scores.scrollIntoViewIfNeeded();
    await expect(scores).toBeInViewport();
  }
  await checkSize(768, 1024);
  await checkSize(1024, 768);
  await checkSize(700, 780);
  await checkSize(600, 900);
  await page.getByRole("link",{name:/chat/i}).tap();
  const input=page.getByRole("textbox",{name:"Message"});
  await input.fill("Tablet draft");
  await page.setViewportSize({width:600,height:380});
  await expect(page.getByRole("button",{name:"Send",exact:true})).toBeInViewport();
  await expect(input).toHaveValue("Tablet draft");
  await page.setViewportSize({width:600,height:900});
  await page.getByRole("link",{name:"Back to board"}).tap();
  await expect(review).toHaveAttribute("data-state","valid");
});
