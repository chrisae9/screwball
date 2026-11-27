import {test, expect} from "@playwright/test";
import {spawn} from "node:child_process";
import {once} from "node:events";
import {mkdtemp, mkdir, readFile, writeFile, rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {createInterface} from "node:readline";
import {randomUUID} from "node:crypto";

test("history filters real results, searches earlier words and sorts scores without losing the selected view", async ({browser}) => {
  const dir = await mkdtemp(join(tmpdir(), "scrabble-history-"));
  let server;
  let exited;
  let context;
  try {
    const seed = JSON.parse(await readFile("e2e/fixture-data/games/1756fa1c-f0b0-48d0-8304-8a79c4230ef6.json", "utf8"));
    const user = JSON.parse(await readFile("e2e/fixture-data/users/1d92bc0a-ff6c-41b7-93da-4309788e3d18.json", "utf8"));
    await Promise.all(["users","games"].map(d => mkdir(join(dir,d))));
    await writeFile(join(dir,"users",user.id+".json"),JSON.stringify(user));
    await Promise.all([["April",120,90,2,false],["Easy bot",135,135,14,true],["Riley",200,201,45,false]].map(async ([name, score, other, days, bot]) => {
      const game = structuredClone(seed);
      game.id = randomUUID();
      game.status = "Finished";
      game.created_at = new Date(Date.now()-days*86400000).toISOString();
      delete game.updated_at; // Old saves only carry creation time.
      game.consecutive_scoreless = 6;
      Object.assign(game.seats[0], {name:user.display_name, kind:{Human:{user_id:user.id}},score});
      Object.assign(game.seats[1], {name, kind:bot?{Bot:{difficulty:"Easy"}}:{Human:{user_id:randomUUID()}},score:other});
      game.moves = [{seat:1,kind:{Play:{placements:[]}},words:["ORBIT"],points:7},{seat:0,kind:"Pass",words:[],points:0}];
      await writeFile(join(dir,"games",game.id+".json"),JSON.stringify(game));
    }));
    server = spawn("./target/debug/screwball", [], {env:{...process.env, PORT:"0",PASSKEY_DISABLED:"1",DATA_PATH:dir},stdio:["ignore","pipe","inherit"]});
    exited = once(server,"exit");
    let baseURL;
    for await (const line of createInterface({input:server.stdout})) {
      const port = line.match(/^listening on port (\d+)$/);
      if (port) {baseURL = "http://127.0.0.1:"+port[1]; break;}
    }
    expect(baseURL).toBeTruthy();
    context = await browser.newContext({baseURL});
    const page = await context.newPage();
    expect((await page.request.post("/auth/login/begin",{data:{username:user.username}})).ok()).toBe(true);
    await page.goto("/history");
    const rows = page.locator(".lobby-row:visible .lobby-name");
    await expect(rows).toHaveText(["April","Easy bot","Riley"]);
    await page.getByRole("button",{name:"Lost",exact:true}).click();
    await expect(rows).toHaveText(["Riley"]);
    await page.getByRole("button",{name:"All",exact:true}).click();
    await page.getByRole("button",{name:"Filters",exact:true}).click();
    await page.getByLabel("Sort by").selectOption("score");
    await expect(rows).toHaveText(["Riley","Easy bot","April"]);
    await page.getByLabel("Sort by").selectOption("closest");
    await expect(rows).toHaveText(["Easy bot","Riley","April"]);
    await page.getByLabel("Opponent").selectOption("people");
    await page.getByLabel("Played").selectOption("7");
    await page.getByRole("searchbox").fill("orbit");
    await expect(rows).toHaveText(["April"]);
    const refresh = page.waitForResponse(r => r.url().endsWith("/api/activity") && r.request().headers()["x-scrabble-history"] === "1");
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await refresh;
    await expect(rows).toHaveText(["April"]);
    await page.getByRole("searchbox").fill("nobody");
    await expect(page.getByRole("heading",{name:"No matching games"})).toBeVisible();
    await page.getByRole("button",{name:"Clear search and filters"}).click();
    await expect(page.getByLabel("Sort by")).toHaveValue("closest");
    await expect(rows).toHaveText(["Easy bot","Riley","April"]);
    const lost=page.getByRole("button",{name:"Lost",exact:true});
    await lost.click();
    await page.getByRole("searchbox").fill("nobody");
    await page.getByRole("button",{name:"Clear search",exact:true}).click();
    await expect(lost).toHaveAttribute("aria-pressed","true");
    await expect(rows).toHaveText(["Riley"]);
    await rows.first().click();
    await expect(page).toHaveURL(/\/games\/[0-9a-f-]+$/);
  } finally {
    await context?.close();
    if (server && server.exitCode === null && server.signalCode === null) server.kill();
    if (exited) await exited;
    await rm(dir,{recursive:true,force:true});
  }
});
