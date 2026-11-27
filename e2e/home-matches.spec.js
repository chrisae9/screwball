import { expect, test } from "@playwright/test";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";

// A public open match on the shared server would appear in other home snapshots.
// Seed a private server with a known AT rack and two open seats for this test.
async function seedHomeMatch(dir) {
  const host = randomUUID();
  const invited = randomUUID();
  const game = JSON.parse(await readFile("e2e/fixture-data/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6.json", "utf8"));
  game.id = randomUUID();
  game.seats[0].kind.Human.user_id = host;
  game.seats[0].name = "Match host";
  game.seats[0].rack = [{Letter:"A"}, {Letter:"T"}];
  const open = game.seats[1];
  game.seats.push({...open, id:randomUUID()}, {...open, id:randomUUID(),
    invitation:{player:invited}, name:"Reserved player (invited)"});
  const folder = join(dir, "games");
  await mkdir(folder);
  await writeFile(join(folder, game.id + ".json"), JSON.stringify(game));
  const users = join(dir, "users");
  await mkdir(users);
  const user = JSON.parse(await readFile("e2e/fixture-data/users/1d92bc0a-ff6c-41b7-93da-4309788e3d18.json", "utf8"));
  await writeFile(join(users, host + ".json"), JSON.stringify({...user, id:host, username:"host", display_name:"Match host"}));
  await writeFile(join(users, invited + ".json"), JSON.stringify({...user, id:invited, username:"reserved", display_name:"Reserved player"}));
  return "/games/" + game.id;
}

async function withLobby(browser, testLobby) {
  const dir = await mkdtemp(join(tmpdir(), "scrabble-home-"));
  const contexts = [];
  let server;
  let exited;
  try {
    const board = await seedHomeMatch(dir);
    server = spawn("./target/debug/screwball", [], {env:{...process.env, PORT:"0", PASSKEY_DISABLED:"1", DATA_PATH:dir}, stdio:["ignore","pipe","inherit"]});
    exited = once(server, "exit");
    let baseURL;
    for await (const line of createInterface({input:server.stdout})) {
      const match = line.match(/^listening on port (\d+)$/);
      if (match) { baseURL = "http://localhost:" + match[1]; break; }
    }
    expect(baseURL).toBeTruthy();
    const [host, guest, spectator] = await Promise.all(Array.from({length:3}, async () => {
      const context = await browser.newContext({baseURL});
      contexts.push(context);
      return context.newPage();
    }));
    expect((await host.request.post("/auth/login/begin", {data:{username:"host"}})).ok()).toBe(true);
    await Promise.all([[guest,"Visitor"], [spectator,"Onlooker"]].map(async ([page, username]) => {
      expect((await page.request.post("/auth/register/begin", {data:{username}})).ok()).toBe(true);
    }));
    await Promise.all([host,guest,spectator].map(page => page.goto("/")));
    await testLobby({host, guest, spectator, board, baseURL});
  } finally {
    await Promise.all(contexts.map(context => context.close()));
    if (server && server.exitCode === null && server.signalCode === null) server.kill();
    if (exited) await exited;
    await rm(dir, {recursive:true, force:true});
  }
}

test("home cards show live scores and joinable seat counts without exposing reserved players", async ({browser}) => {
  await withLobby(browser, async ({host, guest, spectator, board, baseURL}) => {
    await Promise.all([guest,spectator].map(page=>page.getByRole("button",{name:"Open games",exact:true}).click()));
    const openRow = page => page.locator(".open-match").filter({has:page.locator('a[href="'+board+'"]')});
    await expect(openRow(guest)).toContainText("2 open seats");
    await expect(openRow(guest)).not.toContainText("Reserved player");
    await openRow(guest).getByRole("link").click();
    await expect(guest).toHaveURL(baseURL+board);
    await guest.getByRole("link",{name:"Back to games"}).click();
    await guest.getByRole("button",{name:"Open games",exact:true}).click();
    await openRow(guest).getByRole("button",{name:"Join"}).click();
    await expect(guest).toHaveURL(baseURL+board);
    await expect(openRow(spectator)).toContainText("1 open seat",{timeout:12000});
    await expect(openRow(spectator)).toContainText("Visitor");
    await expect(openRow(spectator)).not.toContainText("Reserved player");

    const ownCard = host.locator('#my-games a[href="'+board+'"]');
    const ownScore = ownCard.locator(".lobby-scores strong");
    await expect(ownScore).toHaveText("0");
    const move = await host.request.post(board+"/move", {data:{kind:"play",placements:[
      {row:7,col:7,letter:"A"}, {row:7,col:8,letter:"T"},
    ]}});
    expect(move.ok()).toBe(true);
    await expect(ownScore).toHaveText("4",{timeout:12000});
    await expect(host.locator('[data-group="waiting"] a[href="'+board+'"]')).toContainText("You played AT +4");
    await ownCard.click();
    await expect(host).toHaveURL(baseURL+board);
    await expect(host.locator('#game-island .board .cell.tile')).toHaveCount(2);
  });
});

test("empty lobby tabs offer relevant actions and create a game another player can join", async ({browser}) => {
  await withLobby(browser, async ({host, spectator, baseURL}) => {
    await spectator.getByRole("button", {name:"Your turn", exact:true}).click();
    const empty = spectator.locator(".lobby-empty");
    await expect(empty.getByRole("heading")).toHaveText("You’re all caught up.");
    await expect(empty.getByRole("button")).toBeHidden();
    await empty.getByRole("link", {name:"View my games"}).click();
    await expect(spectator.getByRole("button", {name:"My games", exact:true})).toHaveAttribute("aria-pressed","true");
    const openTab=host.getByRole("button",{name:"Open games",exact:true});
    await openTab.click();
    const noGames=host.locator(".lobby-empty");
    await expect(noGames.getByRole("heading")).toHaveText("No open games.");
    await expect(noGames.getByRole("button")).toBeHidden();
    await host.getByRole("searchbox").fill("missing-player");
    await noGames.getByRole("button",{name:"Clear search"}).click();
    await expect(openTab).toHaveAttribute("aria-pressed","true");
    await noGames.getByRole("link",{name:"Create an open game"}).click();
    await expect(host.getByRole("combobox", {name:"Seat 2", exact:true})).toHaveValue("open");
    await expect(host.getByRole("combobox", {name:"Seat 3", exact:true})).toHaveValue("off");
    await expect(host.getByRole("button",{name:"Create game"})).toBeEnabled();
    await host.getByRole("button",{name:"Create game"}).click();
    await expect(host).toHaveURL(/\/games\/[0-9a-f-]+$/);
    const created=new URL(host.url()).pathname;
    await spectator.reload();
    await spectator.getByRole("button",{name:"Open games",exact:true}).click();
    const createdRow=spectator.locator(".open-match").filter({has:spectator.locator('a[href="'+created+'"]')});
    await createdRow.getByRole("button",{name:"Join"}).click();
    await expect(spectator).toHaveURL(baseURL+created);
  });
});
