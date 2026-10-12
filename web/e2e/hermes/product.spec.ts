import { test, expect, type Page } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { backToList, chatTitle, createAgentViaUi, openAddMenu } from "../helpers/ui";

// Exercise the real product API. No HTTP routes or Agent execution are mocked.
test.use({ serviceWorkers: "block", trace: "off" });
let owned: string[] = [];
let current = "";
let errors: string[] = [];

async function openImport(page: Page) {
  await openAddMenu(page, "匯入 Bot");
  return page.getByRole("dialog", { name: "匯入 Bot", exact: true });
}

test.beforeEach(async ({ page }) => {
  owned = []; errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/");
  await expect(page.getByRole("complementary", { name: "Bot 清單" }).or(page.locator(".chat-header"))).toBeVisible();
  const created = page.waitForResponse(r => r.request().method() === "POST" && new URL(r.url()).pathname === "/api/agents");
  const name = `Web驗收-${Date.now().toString(36)}`;
  await createAgentViaUi(page, name, "核對來源，驗證實際檔案，再交付。網頁驗收專用 Bot。");
  const response = await created;
  expect(response.ok()).toBeTruthy();
  current = (await response.json()).id; owned.push(current);
  await expect(chatTitle(page)).toHaveText(name);
});

test.afterEach(async ({ request }) => {
  for (const id of owned) {
    const response = await request.delete(`/api/agents/${id}`);
    expect(response.ok()).toBeTruthy();
  }
  expect(errors).toEqual([]);
});

test("create, choose model, save native skill, export and import an independent Bot", async ({ page, request }, info) => {
  await page.locator(".chat-identity").click();
  const settings = page.getByRole("dialog", { name: "Bot 設定", exact: true });
  const provider = settings.getByRole("combobox", { name:"供應商", exact: true });
  const publicSettings = await (await request.get("/api/settings")).json();
  const available = publicSettings.providers.find((p: {id:string; selectable:boolean; models:unknown[]}) => p.selectable && p.models.length);
  expect(available).toBeTruthy();
  await provider.selectOption(available.id);
  await settings.getByRole("combobox", { name:"模型", exact: true }).selectOption(available.models[0].id);
  const newName = `Web已設定-${Date.now().toString(36)}`;
  await settings.getByLabel("名稱", { exact: true }).fill(newName);
  await settings.getByRole("tab", { name: /技能/ }).click();
  await settings.getByRole("button", { name: "新增技能", exact: true }).click();
  await settings.getByLabel("技能名稱").fill("web-verified-workflow");
  await settings.getByLabel("什麼時候使用").fill("核對工作成果時");
  await settings.getByLabel("操作步驟").fill("讀回實際檔案，確認內容與使用者要求一致，再提供下載路徑。");
  await settings.getByRole("button", { name: "儲存技能", exact: true }).click();
  // Built-in skills (e.g. deep-research) are listed too; follow the one this test created.
  const mine = settings.locator(".skill-row", { hasText: "web-verified-workflow" });
  await expect(mine).toHaveCount(1);
  await settings.getByRole("button", { name: "停用 web-verified-workflow" }).click();
  await expect(mine).toContainText("已停用");
  await settings.getByRole("button", { name: "儲存並分享 Bot" }).click();
  const sharing = page.getByRole("dialog", { name: "分享 Bot", exact: true });
  await expect(sharing).toContainText("web-verified-workflow · 已停用");
  const downloaded = page.waitForEvent("download");
  await sharing.getByRole("button", { name: "下載分享檔案" }).click();
  const download = await downloaded;
  const file = await download.path();
  expect(file).toBeTruthy();
  const template = JSON.parse(await readFile(file!, "utf8"));
  expect(template.profile.name).toBe(newName);
  const skill = (rows: {name:string; enabled:boolean}[]) => rows.find(row => row.name === "web-verified-workflow");
  expect(skill(template.skills)?.enabled).toBe(false);
  const saved = await (await request.get(`/api/agents/${current}`)).json();
  expect(saved.provider).toBe(available.id);
  expect(saved.model).toBe(available.models[0].id);
  await sharing.getByRole("button", { name: "返回", exact: true }).click();
  const importer = await openImport(page);
  await importer.locator("input[type=file]").setInputFiles({name:"verified.lazyboy.json", mimeType:"application/json", buffer:Buffer.from(JSON.stringify(template))});
  await expect(importer.getByLabel("Bot 名稱")).toHaveValue(`${newName} (2)`);
  const imported = page.waitForResponse(r => r.request().method() === "PUT" && new URL(r.url()).pathname.startsWith("/api/templates/"));
  await importer.getByRole("button", { name: "建立 Bot", exact: true }).click();
  const result = await imported; expect(result.ok()).toBeTruthy();
  const copy = await result.json(); owned.push(copy.id);
  expect(copy.id).not.toBe(current);
  expect(copy.provider).toBe("");
  await expect(importer.getByRole("status")).toContainText(`${template.skills.length} 個技能`);
  await importer.getByRole("button", { name: "開始對話", exact: true }).click();
  await expect(chatTitle(page)).toHaveText(`${newName} (2)`);
  await page.reload();
  if (info.project.name === "mobile") await page.locator(".bot-row-main", {hasText:`${newName} (2)`}).click();
  await expect(chatTitle(page)).toHaveText(`${newName} (2)`);
  const importedSkills = await (await request.post(`/api/agents/${copy.id}/skills`, { data:{action:"list"} })).json();
  expect(skill(importedSkills.skills)?.enabled).toBe(false);
  await page.screenshot({ path:info.outputPath("imported-bot.png"), fullPage:true });
});

test("upload into this computer and manage a future routine from the web", async ({ page, request }, info) => {
  const input = page.locator(".composer-dock input[type=file]");
  const upload = page.waitForResponse(r => r.request().method() === "POST" && new URL(r.url()).pathname.endsWith("/uploads"));
  await input.setInputFiles({name:"web-proof.txt", mimeType:"text/plain", buffer:Buffer.from("WEB_UPLOAD_VERIFIED\n")});
  const response = await upload; expect(response.ok()).toBeTruthy();
  const saved = await response.json();
  await expect(page.locator(".composer-dock")).toContainText("web-proof.txt");
  expect(await (await request.get(saved.url)).text()).toBe("WEB_UPLOAD_VERIFIED\n");
  await page.getByRole("button", { name:"定時任務", exact:true }).click();
  const dialog = page.getByRole("dialog", { name:"定時任務", exact:true });
  await dialog.getByRole("button", { name:/新增定時任務/ }).click();
  await dialog.getByLabel("任務名稱").fill("Web future routine");
  await dialog.getByLabel("要做什麼").fill("驗證資料後交付結果。");
  await dialog.getByLabel("重複", {exact:true}).selectOption("once");
  await dialog.getByLabel("日期", {exact:true}).fill("2030-01-01");
  await dialog.getByLabel("時間", {exact:true}).fill("09:00");
  await dialog.getByRole("button", { name:"儲存任務", exact:true }).click();
  const routine = dialog.locator(".routine-item", { hasText:"Web future routine" });
  await expect(routine).toContainText("已啟用");
  await routine.getByRole("button", { name:"暫停", exact:true }).click();
  await expect(routine).toContainText("已暫停");
  await routine.getByRole("button", { name:"啟用", exact:true }).click();
  await expect(routine).toContainText("已啟用");
  await routine.getByRole("button", { name:"執行紀錄", exact:true }).click();
  await expect(routine).toContainText("尚無執行紀錄");
  await page.screenshot({ path:info.outputPath("routine-and-upload.png"), fullPage:true });
  await routine.getByRole("button", { name:"刪除", exact:true }).click();
  await routine.getByRole("button", { name:"確定刪除", exact:true }).click();
  await expect(dialog.locator(".routine-item")).toHaveCount(0);
});

test("an authorized remote browser logs in and keeps the session after reload", async ({ page }) => {
  // A loopback alias not in the localhost allowlist exercises the real login gate.
  const remote = new URL(process.env.E2E_BASE_URL || "http://127.0.0.1:8790");
  remote.hostname = "127.0.0.2";
  await page.goto(remote.href);
  await expect(page.getByLabel("登入碼")).toBeVisible();
  await page.getByLabel("登入碼").fill("invalid-code");
  await page.getByRole("button", { name:"登入", exact:true }).click();
  await expect(page.getByRole("alert")).toContainText("登入碼不正確");
  const token = (await readFile("../.data/access-token", "utf8")).trim();
  await page.getByLabel("登入碼").fill(token);
  await page.getByRole("button", { name:"登入", exact:true }).click();
  await expect(page.getByRole("complementary", { name:"Bot 清單" })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("complementary", { name:"Bot 清單" })).toBeVisible();
  const cookies = await page.context().cookies();
  const cookie = cookies.find(c => c.name === "lazyhermes_auth");
  expect(cookie?.httpOnly).toBe(true);
  expect(cookie?.sameSite).toBe("Strict");
});

test("model controls and this Bot's MCP settings connect to the native worker", async ({ page }, info) => {
  await backToList(page);
  await page.getByRole("complementary", { name:"Bot 清單" }).getByRole("button", { name:"設定", exact:true }).click();
  const settings = page.getByRole("dialog", { name:"LazyBoy 設定", exact:true });
  if (info.project.name === "desktop") await settings.getByRole("button", { name:"模型", exact:true }).click();
  await settings.getByText("進階設定", { exact:true }).first().click();
  await expect(settings.getByRole("combobox", { name:"推理強度", exact:true })).toBeEnabled();
  await expect(settings.getByRole("combobox", { name:"推理強度", exact:true }).locator("option[value=high]")).toHaveCount(1);
  await expect(settings.getByRole("checkbox", { name:"讀取圖片", exact:true })).toBeEnabled();
  await expect(settings.getByRole("checkbox", { name:"產生圖片", exact:true })).toBeEnabled();
  if (info.project.name === "desktop") await settings.getByRole("button", { name:"工具串接", exact:true }).click();
  const mcp = settings.locator(".mcp-settings");
  await mcp.getByRole("button", { name:/自訂 MCP 服務/ }).click();
  const editor = mcp.getByRole("region", { name:"編輯 MCP 串接", exact:true });
  await editor.getByLabel("名稱", {exact:true}).fill("web-native-probe");
  await editor.getByRole("combobox", {name:"連線方式",exact:true}).selectOption("stdio");
  await editor.getByLabel("啟動程式", {exact:true}).fill("/opt/hermes/.venv/bin/python");
  await editor.getByLabel("參數（每行一個）", {exact:true}).fill("/opt/lazyhermes/desktop_mcp.py");
  await editor.getByRole("button", {name:"儲存串接",exact:true}).click();
  const card = mcp.locator(".mcp-card", {hasText:"web-native-probe"});
  await expect(card).toBeVisible();
  await card.getByRole("button", {name:"測試連線",exact:true}).click();
  await expect(card).toContainText("連線成功", {timeout:75000});
  await card.getByText("查看工具", {exact:true}).click();
  await expect(card).toContainText("desktop_observe");
  await page.screenshot({path:info.outputPath("native-mcp-connected.png"),fullPage:true});
  await card.getByRole("button", {name:"移除",exact:true}).click();
  await card.getByRole("button", {name:"確定移除",exact:true}).click();
  await expect(card).toHaveCount(0);
});
