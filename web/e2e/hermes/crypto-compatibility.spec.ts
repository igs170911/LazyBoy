import { test, expect, type Page } from "@playwright/test";

test.use({ serviceWorkers: "block" });

const uuidV4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

async function fixture(page: Page, legacyCrypto: boolean) {
  const errors: string[] = [];
  const sent: { text: string; request_id: string }[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.addInitScript(legacy => {
    localStorage.setItem("lazyhermes.locale", "zh-Hant");
    (window as any).EventSource = class { close() {} };
    if (legacy) Object.defineProperty(crypto, "randomUUID", { value: undefined });
  }, legacyCrypto);
  const activity = { state: "idle", running: false, queued: false, active_task_ids: [], questions: [] };
  const agents = [
    { id: "crypto-a", name: "相容甲", activity },
    { id: "crypto-b", name: "相容乙", activity },
  ];
  await page.route("**/api/**", async route => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path.endsWith("/messages") && request.method() === "POST") {
      sent.push(request.postDataJSON());
      await route.fulfill({ json: { queued: `task-${sent.length}` } });
      return;
    }
    const agent = agents.find(row => path === `/api/agents/${row.id}`);
    await route.fulfill({ json:
      path === "/api/auth/status" ? { authenticated: true } :
      path === "/api/agents" ? { agents, groups: [] } :
      agent ? { ...agent, transcript: sent.map((row, index) => ({ id: `message-${index}`, role: "user", content: row.text })) } :
      path.endsWith("/activity") ? activity :
      path.endsWith("/files") ? { files: [] } :
      path === "/api/settings" ? { providers: [], active: "", model: "" } :
      { ready: false, state: "stopped", capabilities: {} }
    });
  });
  await page.goto("/");
  await expect(page.locator(".bot-row")).toHaveCount(2);
  return { sent, errors };
}

for (const legacyCrypto of [false, true]) {
  test(`messages reach the API with distinct UUIDs ${legacyCrypto ? "without crypto.randomUUID" : "with browser crypto defaults"}`, async ({ page }) => {
    const { sent, errors } = await fixture(page, legacyCrypto);
    await page.locator(".bot-row").filter({ hasText: "相容甲" }).click();
    const input = page.locator(".composer textarea");
    for (const text of ["第一則訊息", "第二則訊息"]) {
      await input.fill(text);
      await page.getByRole("button", { name: "送出", exact: true }).click();
      await expect.poll(() => sent.map(row => row.text)).toContain(text);
      await expect(input).toHaveValue("");
    }
    expect(sent).toHaveLength(2);
    expect(sent[0].request_id).toMatch(uuidV4);
    expect(sent[1].request_id).toMatch(uuidV4);
    expect(sent[0].request_id).not.toBe(sent[1].request_id);
    expect(errors).toEqual([]);
  });
}
