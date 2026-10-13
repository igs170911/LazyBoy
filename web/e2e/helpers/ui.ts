import { expect, type Page } from "@playwright/test";

export function isPhone(page: Page) {
  return (page.viewportSize()?.width || 1280) <= 860;
}

/** On phones the Bot list and the conversation are separate screens. */
export async function backToList(page: Page) {
  const back = page.getByRole("button", { name: "返回 Bot 清單", exact: true });
  if (await back.isVisible()) await back.click();
  await expect(page.getByRole("complementary", { name: "Bot 清單" })).toBeVisible();
}

export async function openAddMenu(page: Page, item: "新增 Bot" | "匯入 Bot" | "新增分區") {
  await backToList(page);
  await page.getByRole("complementary", { name: "Bot 清單" }).getByRole("button", { name: "新增", exact: true }).click();
  await page.getByRole("menuitem", { name: item, exact: true }).click();
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
