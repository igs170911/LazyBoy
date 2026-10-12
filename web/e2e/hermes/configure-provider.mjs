// Used by the disposable native protocol fixture: real web settings, no API mocks.
import { chromium, expect } from "@playwright/test";

const config = JSON.parse(process.argv[2]);
const browser = await chromium.launch({executablePath:process.env.PLAYWRIGHT_CHROMIUM || "/usr/bin/chromium", args:["--no-sandbox", "--disable-dev-shm-usage"]});
try {
  const page = await browser.newPage({viewport:{width:1280,height:900}, serviceWorkers:"block"});
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.addInitScript(() => localStorage.setItem("lazyhermes.locale", "zh-Hant"));
  await page.goto(config.product_url);
  await page.getByRole("button", {name:"設定", exact:true}).first().click();
  const settings = page.getByRole("dialog", {name:"LazyBoy 設定",exact:true});
  await settings.getByRole("button", {name:"模型",exact:true}).click();
  await settings.getByLabel("供應商", {exact:true}).selectOption("__custom__");
  await settings.getByLabel("供應商名稱", {exact:true}).fill(config.label);
  await settings.getByLabel("API 網址", {exact:true}).fill(config.base_url);
  await settings.getByLabel("API 金鑰", {exact:true}).fill(config.api_key);
  await settings.getByRole("textbox", {name:/^新模型 ID/}).fill(config.model);
  await settings.getByRole("button", {name:"加入模型",exact:true}).click();
  await settings.getByRole("combobox", {name:/^API 介面/}).selectOption(config.protocol);
  await settings.getByRole("checkbox", {name:"讀取圖片",exact:true}).check();
  const saved = page.waitForResponse(response => response.request().method() === "PUT" && new URL(response.url()).pathname === "/api/settings");
  await settings.getByRole("button", {name:"儲存",exact:true}).click();
  const response = await saved;
  expect(response.ok()).toBeTruthy();
  const data = await response.json();
  expect(data.model).toBe(config.model);
  expect(data.protocol).toBe(config.protocol);
  const provider = data.providers.find(item => item.id === data.active);
  expect(provider.label).toBe(config.label);
  expect(provider.selectable).toBe(true);
  expect(provider.models.find(item => item.id === config.model).vision).toBe(true);
  await page.reload();
  expect(errors).toEqual([]);
  process.stdout.write(JSON.stringify({provider:data.active, model:data.model, protocol:data.protocol}));
} finally {
  await browser.close();
}
