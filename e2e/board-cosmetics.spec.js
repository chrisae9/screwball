import {test, expect} from "@playwright/test";
import {randomUUID} from "node:crypto";

const paint = locator => locator.first().evaluate(el => getComputedStyle(el).backgroundImage);

async function shop(page, action, item) {
  const response = await page.request.post("/shop", {form:{action, item}, maxRedirects:0});
  expect(response.status()).toBe(303);
}

function placements(word, rack, row, firstCol) {
  const tiles = [...rack];
  const play = [];
  for (const letter of word) {
    let index = tiles.findIndex(tile => tile.letter === letter);
    if (index === -1) index = tiles.findIndex(tile => tile.is_blank);
    if (index === -1) return null;
    const [tile] = tiles.splice(index, 1);
    play.push({row, col:firstCol + play.length, letter, is_blank:tile.is_blank});
  }
  return play;
}

// Make a horizontal opening and a vertical cross from the actual random deal.
function plan(words, firstRack, secondRack) {
  for (const word of words) {
    const opening = placements(word, firstRack, 7, 7);
    if (!opening) continue;
    for (const tile of opening) {
      for (const cross of words) {
        for (const i of [0, 1]) {
          if (cross[i] !== tile.letter) continue;
          const reply = placements(cross[1-i], secondRack, i === 0 ? 8 : 6, tile.col);
          if (reply) return [opening, reply];
        }
      }
    }
  }
  return null;
}

async function deal(page, other, recipientId) {
  const created = await page.request.post("/games", {form:{seat2:"invite:" + recipientId}, maxRedirects:0});
  expect(created.status()).toBe(303);
  const url = created.headers().location;
  expect((await other.request.post(url + "/join", {form:{},maxRedirects:0})).status()).toBe(303);
  await page.goto(url);
  const words = JSON.parse(await page.locator("#two-letter-words").textContent());
  const getRack = async player => (await (await player.request.get(url + "/state")).json()).your_rack;
  const turns = plan(words, ...await Promise.all([getRack(page), getRack(other)]));
  if (turns) return {url, turns};
  await page.request.post(url + "/abandon");
  return null;
}

async function submit(page, url, moves) {
  expect((await page.request.post(url + "/move", {data:{kind:"play", placements:moves}})).ok()).toBe(true);
}

async function playableDeal(page, other, recipientId) {
  // Actual racks are dealt randomly; retry only when they cannot form a cross.
  for (let attempt = 0; attempt < 8; attempt++) {
    // eslint-disable-next-line no-await-in-loop
    const game = await deal(page, other, recipientId);
    if (game) return game;
  }
  throw new Error("Could not form a two-player opening after eight deals");
}

async function finish(page, other, url) {
  for (const player of [page,other,page,other,page,other]) {
    // eslint-disable-next-line no-await-in-loop
    const result = await player.request.post(url + "/move", {data:{kind:"pass"}});
    expect(result.ok()).toBe(true);
  }
}

async function earnFinishRewards(page, other, recipientId) {
  const {url, turns} = await playableDeal(page, other, recipientId);
  await submit(page, url, turns[0]);
  await submit(other, url, turns[1]);
  await finish(page, other, url);
}

test("two players see their tile finishes and can independently use a classic board", async ({page, browser, baseURL}) => {
  const guest = await browser.newContext({baseURL});
  try {
    const other = await guest.newPage();
    const suffix = randomUUID().slice(0, 8);
    const otherName = "Guest" + suffix;
    await Promise.all([[page,"Host" + suffix],[other,otherName]].map(async ([p,name]) => {
      expect((await p.request.post("/auth/register/begin",{data:{username:name,display_name:name}})).ok()).toBe(true);
    }));
    const [recipient] = await (await page.request.get("/api/players?q=" + otherName)).json();
    // Three naturally completed two-player matches earn both players enough to buy a finish.
    for (let i = 0; i < 3; i++) {
      // eslint-disable-next-line no-await-in-loop
      await earnFinishRewards(page, other, recipient.id);
    }
    await Promise.all([shop(page, "buy", "cobalt"), shop(other, "buy", "cherry")]);
    const {url, turns} = await playableDeal(page, other, recipient.id);
    const board = page.locator("#game-island .board");
    const rack = page.locator("#game-island .rack");
    await expect(rack.locator('button[data-tile-style="cobalt"]')).toHaveCount(7);
    await board.locator('[data-row="7"][data-col="7"]').click();
    await rack.locator("button[data-tile-id]:not(.tile-blank)").first().click();
    await expect(board.locator(".pending-cell")).toHaveAttribute("data-tile-style", "cobalt");
    await page.getByRole("button", {name:"Clear",exact:true}).click();
    await submit(page, url, turns[0]);
    await submit(other, url, turns[1]);
    await other.goto(url);
    const otherBoard = other.locator("#game-island .board");
    const cobalt = otherBoard.locator('.tile[data-tile-style="cobalt"]');
    const cherry = otherBoard.locator('.tile.last-play[data-tile-style="cherry"]');
    await expect(cobalt).toHaveCount(2);
    await expect(cherry).toHaveCount(1);
    await expect(other.locator('.rack button[data-tile-style="cherry"]')).toHaveCount(7);
    await expect(other.locator(".rack")).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
    await expect(other.locator(".rack")).toHaveCSS("background-image", "none");
    const cobaltPaint = await paint(cobalt);
    expect(cobaltPaint).not.toBe(await paint(cherry));
    await other.getByRole("button", {name:"Zoom in",exact:true}).click();
    await expect(other.getByRole("button", {name:"Zoom out",exact:true})).toBeVisible();
    expect(await paint(cobalt)).toBe(cobaltPaint);

    await page.route("**/public/game.js*", route => route.abort());
    await page.reload();
    const fallback = page.locator("#ssr-fallback .board");
    await expect(fallback.locator('.tile[data-tile-style="cobalt"]')).toHaveCount(2);
    await expect(fallback.locator('.tile[data-tile-style="cherry"]')).toHaveCount(1);
    await page.unroute("**/public/game.js*");
    await shop(page, "uniform", "on");
    await page.reload();
    await expect(board.locator('.tile[data-tile-style="classic"]')).toHaveCount(3);
    await expect(rack.locator('button[data-tile-style="cobalt"]')).toHaveCount(7);
    await board.locator('[data-row="9"][data-col="8"]').click();
    await rack.locator("button[data-tile-id]:not(.tile-blank)").first().click();
    await expect(board.locator(".pending-cell")).toHaveAttribute("data-tile-style", "classic");
    await shop(other, "equip", "classic");
    await shop(page, "uniform", "off");
    await page.reload();
    await expect(board.locator('.tile:not(.pending-cell)[data-tile-style="cobalt"]')).toHaveCount(2);
    await expect(board.locator('.tile:not(.pending-cell)[data-tile-style="classic"]')).toHaveCount(1);
    await finish(page, other, url);
    await page.reload();
    await expect(page.locator(".results .finish-reward")).toHaveText("+20 coins for finishing");
    await other.reload();
    await expect(other.locator(".results .finish-reward")).toHaveText("+20 coins for finishing");
  } finally { await guest.close(); }
});
