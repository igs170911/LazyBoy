import { expect, type Page } from "@playwright/test";

export function isPhone(page: Page) {
  return (page.viewportSize()?.width || 1280) <= 860;
}

/**
 * On phones the Bot list and the conversation are separate screens. Both stay mounted and the one slid away is
 * only moved off screen, where Playwright still counts its buttons as visible, so ask the app which one shows.
 */
export async function backToList(page: Page) {
  const inChat = page.locator(".app.is-mobile[data-view=chat]");
  if (await inChat.count()) await page.getByRole("button", { name: "返回 Bot 清單", exact: true }).click();
  await expect(inChat).toHaveCount(0);
  await expect(page.getByRole("complementary", { name: "Bot 清單" })).toBeVisible();
}

/** The sidebar's "新增" opens one dialog (Grok's "New chat") with a new bot, an import and a new section. */
export async function openAddMenu(page: Page, item: "新增 Bot" | "匯入 Bot" | "新增分區") {
  await backToList(page);
  await page.getByRole("complementary", { name: "Bot 清單" }).getByRole("button", { name: "新增", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "新增", exact: true });
  const label = { "新增 Bot": "建立新的 Bot", "匯入 Bot": "匯入 Bot", "新增分區": "新增側欄分區" }[item];
  await dialog.getByRole("button", { name: label, exact: true }).click();
}

export async function createAgentViaUi(page: Page, name: string, persona: string) {
  await openAddMenu(page, "新增 Bot");
  const form = page.getByRole("dialog", { name: "新增 Bot", exact: true });
  await form.getByLabel("名稱", { exact: true }).fill(name);
  await form.getByLabel("人設", { exact: true }).fill(persona);
  await form.getByRole("button", { name: "建立", exact: true }).click();
  await expect(form).toHaveCount(0);
}

export function chatTitle(page: Page) {
  return page.locator(".chat-header .chat-title");
}
