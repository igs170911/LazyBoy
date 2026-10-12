// Read-only final screenshot of the running product and its actual desktop.
import { chromium, expect } from "@playwright/test";
import { mkdir, writeFile } from "node:fs/promises";

const browser = await chromium.launch({executablePath:process.env.PLAYWRIGHT_CHROMIUM || "/usr/bin/chromium", args:["--no-sandbox", "--disable-dev-shm-usage"]});
const proof = [];
try {
  await mkdir("../docs/screenshots", {recursive:true});
  for (const mobile of [false,true]) {
    const page = await browser.newPage({viewport:mobile ? {width:390,height:844} : {width:1440,height:900}, isMobile:mobile, hasTouch:mobile, serviceWorkers:"block"});
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.addInitScript(() => localStorage.setItem("lazyhermes.locale", "zh-Hant"));
    await page.goto("http://localhost:8790");
    const bot = page.locator(".bot-row-main", {hasText:"LazyBoy"}).first();
    await expect(bot).toBeVisible({timeout:15000});
    await bot.click();
    await expect(page.locator(".chat-title")).toContainText("LazyBoy");
    await expect(page.locator("body")).toContainText("HERMES_WORKS_20261007");
    await page.getByRole("button", {name:"電腦",exact:true}).first().click();
    await expect(page.locator("iframe.desktop-frame")).toBeVisible({timeout:30000});
    await expect.poll(async () => page.locator("iframe.desktop-frame").evaluate(frame => frame.contentDocument?.getElementById("noVNC_status")?.textContent || ""), {timeout:30000}).toMatch(/Connected|已連線|连接/i);
    expect(errors).toEqual([]);
    const kind = mobile ? "mobile" : "desktop";
    await page.screenshot({path:`../docs/screenshots/${kind}-ready.png`, fullPage:true});
    proof.push({device:kind, native_transcript:true, actual_rfb_connection:true, page_errors:errors});
    await page.close();
  }
  await writeFile("../.run/handoff-web-proof.json", JSON.stringify({passed:true,checks:proof},null,2)+"\n");
  process.stdout.write(JSON.stringify(proof));
} finally {await browser.close();}
