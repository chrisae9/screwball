import {test, expect} from "@playwright/test";
import {randomUUID} from "node:crypto";
import {spawn} from "node:child_process";
import {once} from "node:events";
import {mkdtemp, mkdir, readFile, writeFile, rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {createInterface} from "node:readline";

const tiles = [["koi",90],["circuit",125],["kintsugi",200],["rift",275],["serpent",0],["orrery",0]];

const rules = {
  vault101: /VAULT, GEAR or SHELTER/,
  pipboy: /RAD, SCAN or BYTE/,
  nukacola: /COLA, CAP or FIZZ/,
  kirby: /STAR, DREAM or PUFF/,
  jett: /DASH, WIND or GUST/,
  sage: /JADE, WALL or HEAL/,
  volleyball: /SET, ACE or DIG/,
  tracer: /TIME, BLINK or ZIP/,
  dva: /BOOST, MECH or FLY/,
  calico: /PATCH, COAT or PURR/,
  tuxedo: /TUX, PAWS or SOCK/,
  mooncat: /MOON, PROWL or NIGHT/,
  moth: /blank/i,
  junction: /3 words/i,
  kraken: /8.?(?:letter|or more letters)/i,
  fox: /palindrome/i,
  scarab: /J.*Q.*X.*Z/i,
  phoenix: /3 (?:different |distinct )?games/i,
};
const themed = ["kirby","jett","sage","volleyball","tracer","dva","calico","tuxedo","mooncat"];
const fallout = ["vault101", "pipboy", "nukacola"];
const original = ["moth","junction","kraken","fox","scarab","phoenix"];

function historicGame(template, opponent, turns) {
  const game = structuredClone(template);
  game.id = randomUUID();
  game.seats[1].kind = {Human:{user_id:opponent.id}};
  game.seats[1].name = opponent.display_name;
  game.moves = [];
  for (const {letters, row, col, blank, words, points} of turns) {
    if (game.moves.length) game.moves.push({seat:1,kind:"Pass",words:[],points:0});
    const placements = [...letters].map((letter, i) => {
      const tile = {letter, is_blank:i === blank};
      game.board.squares[row*15+col+i].tile = tile;
      return {letter, is_blank:tile.is_blank, position:{row,col:col+i}};
    });
    // Original saves have no played_on date and no new achievement fields.
    game.moves.push({seat:0,kind:{Play:{placements}},words,points});
    game.seats[0].score += points;
  }
  game.turn = 1;
  return game;
}

async function seed(dir) {
  const fixture = "e2e/fixture-data";
  const game = JSON.parse(await readFile(fixture + "/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6.json","utf8"));
  const user = JSON.parse(await readFile(fixture + "/users/1d92bc0a-ff6c-41b7-93da-4309788e3d18.json","utf8"));
  // Seed the saved reward ledger. The test buys and equips every item through the real shop.
  const finishes = Object.fromEntries(Array.from({length:42}, () => [randomUUID(),"human"]));
  const daily = Array.from({length:14}, (_,i) => "2025-06-" + String(i+1).padStart(2,"0"));
  user.settings = {cosmetics:{rewards:{finishes,daily,best_play:75}}};
  const opponent = {...structuredClone(user), id:randomUUID(), username:"robin", display_name:"Robin"};
  const archives = [
    ...["STAR","GUST","WALL","ACE","BLINK","MECH","PURR","SOCK","MOON","GEAR","RAD","COLA"].map(letters =>
      [{letters, row:7,col:7,words:[letters],points:10}]),
    [{letters:"JAZZILY", row:7,col:4, blank:3,words:["JAZZILY"],points:100}],
    [{letters:"EQUINOX", row:7,col:4, words:["EQUINOX"],points:96}],
    [{letters:"EXAMPLE", row:7,col:4, words:["EXAMPLE"],points:86},
      {letters:"S",       row:7,col:11,words:["EXAMPLES"],points:20}],
    [{letters:"LEVEL", row:7,col:5,words:["LEVEL"],points:16}],
    [{letters:"AT",row:7,col:7,words:["AT"],points:4},
      {letters:"HO",row:8,col:7,words:["HO","AH","TO"],points:12}],
  ].map(turns => historicGame(game, opponent, turns));
  const placements = [..."CAT"].map((letter, i) => {
    const tile = {letter, is_blank:letter === "A"};
    game.board.squares[7*15+6+i].tile = tile;
    return {letter, is_blank:tile.is_blank, position:{row:7,col:6+i}};
  });
  game.seats[0].score = 8;
  game.seats[1].kind = {Human:{user_id:opponent.id}};
  game.seats[1].name = "Robin";
  game.moves = [{seat:0,kind:{Play:{placements}},words:["CAT"],points:8}, {seat:1,kind:"Pass",words:[],points:0}];
  await Promise.all(["games","users"].map(d=>mkdir(join(dir,d))));
  await Promise.all([[game,"games"],[user,"users"],[opponent,"users"],...archives.map(g=>[g,"games"])].map(([entity,folder]) =>
    writeFile(join(dir,folder,entity.id + ".json"), JSON.stringify(entity))));
  return {game, user, opponent};
}

const paint = item => item.first().evaluate(el => getComputedStyle(el).backgroundImage);

async function verifyFinishes(page, browser, achievements, priced) {
  const dir = await mkdtemp(join(tmpdir(), "scrabble-rare-"));
  let server, exited;
  try {
    const {game, user, opponent} = await seed(dir);
    server = spawn("./target/debug/screwball", [], {env:{...process.env, PORT:"0", PASSKEY_DISABLED:"1", DATA_PATH:dir},stdio:["ignore","pipe","inherit"]});
    exited = once(server,"exit");
    let base;
    for await (const line of createInterface({input:server.stdout})) {
      const port = line.match(/^listening on port (\d+)$/);
      if (port) { base = "http://127.0.0.1:" + port[1]; break; }
    }
    expect(base).toBeTruthy();
    expect((await page.request.post(base + "/auth/login/begin",{data:{username:user.username}})).ok()).toBe(true);

    // Both players have the same ample coin balance and share the saved games.
    // Only the player whose stored moves qualified may equip the challenge finishes.
    const visitor = await browser.newContext({baseURL:base});
    try {
      expect((await visitor.request.post("/auth/login/begin",{data:{username:opponent.username}})).ok()).toBe(true);
      const locked = await visitor.newPage();
      await locked.goto("/shop?tab=collection");
      await expect(locked.locator(".coin-balance")).toContainText("910 coins");
      await Promise.all(achievements.map(async id => {
        await expect(locked.locator('a.shop-card[href*="item=' + id + '"]')).toHaveCount(0);
        for (const action of ["buy","equip"]) {
          // eslint-disable-next-line no-await-in-loop
          const result = await visitor.request.post("/shop",{form:{action,item:id},maxRedirects:0});
          expect(result.status(), action + " locked " + id).toBe(422);
        }
      }));
      const first = achievements[0];
      await locked.goto("/shop?item=" + first);
      await expect(locked.locator(".unlock-rule")).toContainText(rules[first]);
      await expect(locked.locator(".finish-info button")).toHaveCount(0);
      await locked.goto("/shop");
      await Promise.all(achievements.map(id =>
        expect(locked.locator('a.shop-card[href*="item=' + id + '"]')).toContainText(rules[id])
      ));
    } finally { await visitor.close(); }
    await page.goto(base + "/shop?tab=collection");
    await Promise.all(achievements.map(id =>
      expect(page.locator('a.shop-card[href*="item=' + id + '"]')).toContainText("Owned")
    ));

    let balance = 910; // 42 completed multiplayer games plus 14 scoring dates.
    const paints = new Set();
    async function choose(id, cost) {
      await page.goto(base + "/shop?item=" + id);
      if (rules[id]) await expect(page.locator(".unlock-rule")).toContainText(rules[id]);
      await page.getByRole("button",{name:cost ? /Unlock.*equip/i : "Equip",exact:!cost}).click();
      balance -= cost;
      await expect(page.locator(".coin-balance")).toContainText(balance + " coins");
      await expect(page.getByRole("status")).toContainText("Equipped");
      await page.goto(base + "/games/" + game.id);
    }
    async function checkTile([id, cost]) {
      await choose(id, cost);
      const board = page.locator("#game-island .board");
      const tile = board.locator('.cell.tile[data-tile-style="' + id + '"]');
      const key = page.locator('.rack button[data-tile-style="' + id + '"]:not(.tile-blank)').first();
      await expect(tile).toHaveCount(3);
      await expect(tile.first()).toHaveClass(/last-play/);
      const blank = tile.filter({has:page.locator(".tile-blank")});
      await expect(blank.locator(".tile-points")).toHaveText("0");
      await expect(blank.locator(".tile-letter")).toHaveCSS("text-decoration-line","underline");
      const finish = await paint(tile);
      const artLoads = await tile.first().evaluate(el => {
        const layers = [...getComputedStyle(el).backgroundImage.matchAll(/url\("([^"]+)"\)/g)];
        return Promise.all(layers.map(async ([, src]) => {
          const image = new Image();
          image.src = src;
          await image.decode();
          return image.naturalWidth > 0 && image.naturalHeight > 0;
        }));
      });
      expect(artLoads, id + " artwork must load on the board").toContain(true);
      expect(await paint(key)).toBe(finish);
      paints.add(finish);
      await board.locator('[data-row="7"][data-col="9"]').click();
      await key.click();
      const draft = board.locator(".pending-cell");
      await expect(draft).toHaveAttribute("data-tile-style", id);
      await expect(draft.locator(".tile-face")).toHaveCSS("outline-style","dashed");
      expect(await paint(draft.locator(".tile-face"))).toBe(finish);
      const geometry = await draft.boundingBox();
      expect(Math.abs(geometry.width - geometry.height)).toBeLessThan(.25);
      await page.getByRole("button",{name:"Clear",exact:true}).click();
    }
    for (const item of [...priced, ...achievements.map(id => [id,0])]) {
      // Each selection spends this player's balance and changes the same board.
      // eslint-disable-next-line no-await-in-loop
      await checkTile(item);
    }
    expect(paints.size).toBe(priced.length + achievements.length);
  } finally {
    if (server && server.exitCode === null && server.signalCode === null) server.kill();
    if (exited) await exited;
    await rm(dir,{recursive:true,force:true});
  }
}
for (const [group, achievements, priced] of [["Fallout finishes",fallout,[]], ["word challenges",themed,[]], ["original finishes",original,tiles]]) {
  test("shop credits legacy moves and equips " + group + " on letters, blanks and drafts", async ({page, browser}) => {
    await verifyFinishes(page, browser, achievements, priced);
  });
}
