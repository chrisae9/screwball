import {expect, test} from "@playwright/test";

const openGame = "/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6";
const playedGame = "/games/13b7c864-5e32-4f40-b99e-b1b34171c832";
const sizes = [
  {width:320, height:568}, {width:390, height:844},
  {width:820, height:1180}, {width:1180, height:820},
  {width:1440, height:900},
];

async function checkBoard(board) {
  const geometry = await board.evaluate(grid => {
    const {width, height} = grid.getBoundingClientRect();
    const cells = [...grid.querySelectorAll(".cell")].map(cell => cell.getBoundingClientRect());
    return {
      count: cells.length, shortest: Math.min(...cells.map(cell => cell.width)),
      boardError: Math.abs(width - height),
      cellError: Math.max(...cells.map(cell => Math.abs(cell.width - cell.height))),
      rowWidth: cells[14].right - cells[0].left,
      columnHeight: cells[224].bottom - cells[14].top,
    };
  });
  expect(geometry.count).toBe(225);
  expect(geometry.shortest).toBeGreaterThan(10);
  expect(geometry.boardError).toBeLessThanOrEqual(.25);
  expect(geometry.cellError).toBeLessThanOrEqual(.25);
  expect(Math.abs(geometry.rowWidth - geometry.columnHeight)).toBeLessThanOrEqual(.25);
}

async function rackGeometry(rack) {
  return rack.evaluate(el => {
    const tiles = [...el.querySelectorAll(".rack-tile")].map(tile => tile.getBoundingClientRect()).filter(rect => rect.width);
    return {
      count:tiles.length, first:tiles[0].x, side:tiles[0].width,
      widthRange: Math.max(...tiles.map(tile => tile.width)) - Math.min(...tiles.map(tile => tile.width)),
      error: Math.max(...tiles.map(tile => Math.abs(tile.width - tile.height))),
      height: el.getBoundingClientRect().height,
      controls: el.parentElement.querySelector(".controls").getBoundingClientRect().top - el.getBoundingClientRect().top,
    };
  });
}

for (const viewport of sizes) {
  test.describe(`tile geometry at ${viewport.width} × ${viewport.height}`, () => {
    test.use({viewport});

    test("all 225 occupied and empty squares fit before and after hydration", async ({page}) => {
      await page.request.post("/auth/login/begin", {data:{username:"scott"}});
      await page.route("**/public/game.js*", route => route.abort());
      await page.goto(playedGame);
      const fallback = page.locator("#ssr-fallback .board");
      await expect(fallback.locator(".tile").first()).toBeVisible();
      await checkBoard(fallback);

      await page.unroute("**/public/game.js*");
      await page.reload();
      const board = page.locator("#game-island .board");
      await expect(board.locator(".tile").first()).toBeVisible();
      await checkBoard(board);
    });

    test("rack tiles, placeholders and the action row stay in place while drafting", async ({page}) => {
      await page.request.post("/auth/login/begin", {data:{username:"shelli"}});
      await page.goto(openGame);
      const board = page.locator("#game-island .board");
      const rack = page.locator("#game-island .rack");
      const letter = rack.getByRole("button").filter({has:page.getByText("F",{exact:true})});
      await expect(letter).toBeVisible();
      const resting = await rackGeometry(rack);
      expect(resting.count).toBe(7);
      expect(resting.error).toBeLessThanOrEqual(.25);
      await board.locator('[data-row="7"][data-col="7"]').click();
      await letter.click();
      await expect(board.locator(".pending-cell")).toHaveCount(1);
      await expect(rack.locator(".rack-slot")).toHaveCount(1);
      await checkBoard(board);
      const drafting = await rackGeometry(rack);
      const hasUndo = await page.evaluate(() => matchMedia("(max-width:480px), (any-pointer:coarse)").matches);
      expect(drafting.count).toBe(hasUndo ? 8 : 7);
      expect(drafting.error).toBeLessThanOrEqual(.25);
      expect(drafting.widthRange).toBeLessThanOrEqual(.25);
      for (const dimension of ["first", "side", "controls", "height"]) {
        expect(Math.abs(drafting[dimension] - resting[dimension]), dimension).toBeLessThanOrEqual(.25);
      }
      await page.getByRole("button", {name:"Clear",exact:true}).click();
      await expect(board.locator(".pending-cell")).toHaveCount(0);
      expect((await rackGeometry(rack)).controls).toBeCloseTo(resting.controls, 1);
    });
  });
}
