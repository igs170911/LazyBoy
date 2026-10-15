import { test, expect, type Page, type Route } from "@playwright/test";

// Drives the workspace with a scripted API and event stream, so live-work states
// (streaming, steps, questions, steering, stop) are checked deterministically.
test.use({ serviceWorkers: "block", trace: "off" });

type Json = Record<string, unknown>;
const AGENT = "fixture-bot";
const IDLE = { state: "idle", running: false, queued: false, observed_at_ms: 0, active_task_ids: [] };

function setup() {
  const state = {
    activity: { ...IDLE } as Json,
    transcript: [
      { id: "m1", role: "user", content: "幫我整理今天的新聞", at: "2026-10-08T01:00:00Z" },
      { id: "delta:t0", role: "assistant", content: "好的，這是今天的三則重點。", at: "2026-10-08T01:01:00Z" },
    ] as Json[],
    tasks: {
      t0: { id: "t0", state: "terminal", goal: "news", verdict: "answer", artifacts: [] },
      t1: { id: "t1", state: "running", goal: "report", started_at: new Date().toISOString(), artifacts: [] },
    } as Record<string, Json>,
    files: [] as Json[],
    posts: [] as { path: string; body: Json }[],
    threads: [{ id: AGENT, title: "主要對話", main: true, preview: "", message_count: 2, running: false, queued: false }] as Json[],
    threadTranscripts: {} as Record<string, Json[]>,
    requests: [] as { method: string; path: string; body?: Json }[],
    lesson: null as Json | null,
    lessons: {} as Record<string, Json>,
  };
  const row = () => ({ id: AGENT, name: "測試 Bot", title: "驗收用", avatar_color: "#9159FE", avatar_shape: "round", last_at: "2026-10-08T01:01:00Z", preview: "好的", activity: state.activity, skill_count: 0 });
  async function handle(route: Route) {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    const json = (body: unknown, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
    if (request.method() === "POST") state.posts.push({ path, body: JSON.parse(request.postData() || "{}") });
    if (request.method() !== "GET") state.requests.push({ method: request.method(), path, body: request.postData() ? JSON.parse(request.postData()!) : undefined });
    if (path === "/api/auth/status") return json({ authenticated: true });
    if (path === "/api/agents" && request.method() === "GET") return json({ agents: [row()], groups: [] });
    if (path === "/api/rooms" && request.method() === "GET") return json({ rooms: [] });
    if (path === `/api/agents/${AGENT}`) {
      const thread = url.searchParams.get("thread");
      const transcript = thread ? state.threadTranscripts[thread] || [] : state.transcript;
      return json({ ...row(), expertise: "", transcript, running: state.activity.running, activity: state.activity });
    }
    if (path === `/api/agents/${AGENT}/threads` && request.method() === "GET") return json({ threads: state.threads });
    if (path === `/api/agents/${AGENT}/threads` && request.method() === "POST") {
      const created = { id: `thread-${state.threads.length}`, title: "新對話", main: false, preview: "", message_count: 0, running: false, queued: false };
      state.threads.push(created);
      return json(created);
    }
    if (path.startsWith(`/api/agents/${AGENT}/threads/`)) {
      const id = path.split("/").pop()!;
      if (request.method() === "DELETE") { state.threads = state.threads.filter(row => row.id !== id); return json({ ok: true }); }
      const target = state.threads.find(row => row.id === id)!;
      target.title = JSON.parse(request.postData() || "{}").title;
      return json(target);
    }
    if (path === `/api/agents/${AGENT}/activity`) return json(state.activity);
    if (path === `/api/agents/${AGENT}/files`) return json({ files: state.files });
    if (path === `/api/agents/${AGENT}/files/content`) return route.fulfill({ status: 200, contentType: "text/markdown", body: "# 報告\n\n驗收內容 FIXTURE_REPORT" });
    if (path === `/api/agents/${AGENT}/files/stat`) {
      const asked = url.searchParams.get("path") || "";
      if (/\/poc\/?$/.test(asked)) return json({ kind: "folder", truncated: false, entries: [{ name: "qa", path: "/workspace/poc/qa", dir: true, bytes: null }, { name: "log.md", path: "/workspace/poc/log.md", dir: false, bytes: 1200 }] });
      if (asked.endsWith("server.py")) return json({ kind: "missing", elsewhere: [{ agent_id: "lead-bot", name: "技術主管" }] });
      return json({ kind: "file", bytes: 40 });
    }
    if (path.startsWith("/api/tasks/")) return json(state.tasks[path.split("/").pop()!] || {}, state.tasks[path.split("/").pop()!] ? 200 : 404);
    if (path === "/api/media") return json({ model: "m", capabilities: { images: true, pdf: true } });
    if (path === `/api/agents/${AGENT}/messages`) return json({ steered: state.activity.running ? "t1" : undefined, queued: state.activity.running ? undefined : "t2" });
    if (path === `/api/agents/${AGENT}/teach`) {
      if (request.method() === "GET") return json({ lesson: state.lesson });
      const body = JSON.parse(request.postData() || "{}");
      if (body.action === "start") state.lesson = { id: "L1", agent_id: AGENT, goal: body.goal, trial: body.trial, state: "recording", steps: [], base_url: `/api/agents/${AGENT}/files/content?path=lessons/L1/` };
      else if (body.action === "pause" || body.action === "resume") state.lesson = { ...state.lesson, state: body.action === "pause" ? "paused" : "recording" };
      else if (body.action === "note") state.lesson = { ...state.lesson, steps: [...(state.lesson!.steps as Json[]), { id: "n1", at: 3, source: "user", kind: "note", text: `使用者補充：${body.text}` }] };
      else if (body.action === "drop") state.lesson = { ...state.lesson, steps: (state.lesson!.steps as Json[]).filter(step => step.id !== body.step) };
      else if (body.action === "finish") {
        state.lessons.L1 = { ...state.lesson, state: "finished" };
        state.lesson = null;
        state.transcript = [...state.transcript, { id: "m9", role: "user", content: "我示範了「查訂單」，共 2 步，請學起來", lesson_id: "L1", at: "2026-10-08T01:10:00Z" }];
        return json({ lesson: null, queued: "t5" });
      } else if (body.action === "cancel") state.lesson = null;
      return json({ lesson: state.lesson });
    }
    if (path.startsWith(`/api/agents/${AGENT}/lessons/`)) return json({ lesson: state.lessons[path.split("/").pop()!] });
    if (path === `/api/agents/${AGENT}/computer`) return json({ ready: false, viewer_url: "", error: "測試環境沒有電腦" });
    if (path.startsWith(`/api/agents/${AGENT}/`)) return json({ ok: true });
    return json({});
  }
  return { state, handle };
}

async function open(page: Page, fixture: ReturnType<typeof setup>) {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.addInitScript(() => {
    localStorage.setItem("lazyhermes.active", "fixture-bot");
    const sources: { onmessage?: (event: { data: string }) => void; onopen?: () => void; url: string }[] = [];
    (window as unknown as { __sources: typeof sources }).__sources = sources;
    (window as unknown as { EventSource: unknown }).EventSource = class {
      url: string; onmessage?: (event: { data: string }) => void; onopen?: () => void;
      constructor(url: string) { this.url = url; sources.push(this); }
      close() { const index = sources.indexOf(this); if (index >= 0) sources.splice(index, 1); }
    };
  });
  await page.route("**/api/**", fixture.handle);
  await page.goto("/");
  await page.waitForLoadState("domcontentloaded");
  if ((page.viewportSize()?.width || 1280) <= 860) await page.locator(".bot-row-main", { hasText: "測試 Bot" }).click();
  await expect(page.locator(".chat-title")).toHaveText("測試 Bot");
}

let nextEvent = 1;
async function emit(page: Page, events: { kind: string; task_id?: string; payload: Json }[]) {
  await expect.poll(() => page.evaluate(() => (window as unknown as { __sources: unknown[] }).__sources.length)).toBeGreaterThan(0);
  await page.evaluate(batch => {
    for (const source of (window as unknown as { __sources: { onmessage?: (event: { data: string }) => void }[] }).__sources) source.onmessage?.({ data: JSON.stringify({ events: batch }) });
  }, events.map(event => ({ id: nextEvent++, ...event })));
}

test("live work streams steps and text, accepts steering, stops, and shows results", async ({ page }) => {
  const fixture = setup();
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await open(page, fixture);
  await expect(page.locator(".msg.assistant")).toContainText("今天的三則重點");

  fixture.state.activity = { state: "running", running: true, queued: false, observed_at_ms: 1, active_task_ids: ["t1"], latest_task_id: "t1" };
  await emit(page, [
    { kind: "m0.progress", task_id: "t1", payload: { message: "正在打開來源網站" } },
    { kind: "runtime", task_id: "t1", payload: { type: "tool_started", id: "s1", name: "browser_navigate", detail: "https://example.com/news" } },
  ]);
  const card = page.getByRole("region", { name: "工作進度" });
  await expect(card).toContainText("思考中");
  await expect(card).toContainText("正在打開來源網站");
  await expect(card.locator(".step-list li[data-state=running]")).toContainText("操作瀏覽器");
  await expect(page.locator(".chat-status")).toContainText("工作中");
  // The bot's face replaces the spinner, and its eyes do what it is doing: reading along while it browses.
  await expect(card.locator(".think-toggle .avatar")).toHaveAttribute("data-motion", "read");
  await expect(page.locator(".chat-identity .avatar")).toHaveAttribute("data-motion", "read");
  const eyeX = () => card.locator(".think-toggle .mo-eyes").evaluate(el => (el as SVGElement).style.getPropertyValue("--mo-gz-dx1"));
  await page.waitForTimeout(300);
  expect(await eyeX()).toBe(""); // this page asks for reduced motion: the eyes stay put
  await page.emulateMedia({ reducedMotion: "no-preference" });

  await emit(page, [
    { kind: "runtime", task_id: "t1", payload: { type: "tool_finished", id: "s1", name: "browser_navigate", success: true } },
    { kind: "runtime", task_id: "t1", payload: { type: "message_delta", id: "t1", delta: "找到兩則" } },
    { kind: "runtime", task_id: "t1", payload: { type: "message_delta", id: "t1", delta: "相關新聞。" } },
  ]);
  await expect(card.locator(".step-list li[data-state=done]")).toContainText("操作瀏覽器");
  await expect(card.locator(".think-toggle .avatar")).toHaveAttribute("data-motion", "think");
  await expect.poll(eyeX).not.toBe("");
  const rolling = await eyeX();
  await expect.poll(eyeX).not.toBe(rolling);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect(page.locator(".msg.assistant.streaming")).toContainText("找到兩則相關新聞。");

  const composer = page.getByRole("textbox", { name: "訊息" });
  await expect(composer).toHaveAttribute("placeholder", /補充說明給 測試 Bot/);
  await composer.fill("只要台灣的新聞");
  await composer.press("Enter");
  await expect.poll(() => fixture.state.posts.find(post => post.path.endsWith("/messages"))?.body.text).toBe("只要台灣的新聞");
  await expect(card).toContainText("已把你的補充送進目前的工作");
  await expect(composer).toHaveValue("");

  await card.getByRole("button", { name: "停止" }).click();
  await expect.poll(() => fixture.state.posts.some(post => post.path.endsWith("/stop"))).toBe(true);

  fixture.state.activity = { ...IDLE, latest_task_id: "t1" };
  fixture.state.tasks.t1 = { id: "t1", state: "terminal", goal: "report", verdict: "answer", artifacts: [{ id: "a1", kind: "file", title: "report.md", rel_path: "report.md", media_type: "text/markdown", bytes: 2048, status: "available", ok_on_disk: true, created_at: "2026-10-08T01:05:00Z" }] };
  fixture.state.files = [{ path: "report.md", title: "report.md", media_type: "text/markdown", bytes: 2048, updated_at: "2026-10-08T01:05:00Z" }];
  fixture.state.transcript = [...fixture.state.transcript, { id: "local-steer", role: "user", content: "只要台灣的新聞", at: "2026-10-08T01:03:00Z" }, { id: "delta:t1", role: "assistant", content: "找到兩則相關新聞。報告在 report.md。", at: "2026-10-08T01:05:00Z" }];
  await emit(page, [{ kind: "runtime", task_id: "t1", payload: { type: "turn_ended", verdict: "answer" } }]);
  await expect(card).toHaveCount(0);
  const chip = page.locator(".artifact", { hasText: "report.md" });
  await expect(chip).toBeVisible();
  await expect(page.locator(".task-steps")).toContainText("1 個步驟");
  await chip.click();
  await expect(page.getByRole("complementary", { name: "檔案" })).toContainText("FIXTURE_REPORT");
  expect(errors).toEqual([]);
});

test("a long question with many choices scrolls on its own and keeps its send button in reach", async ({ page }) => {
  const fixture = setup();
  await open(page, fixture);
  const long = "請先看完這段背景說明。".repeat(40);
  const questions = Array.from({ length: 5 }, (_, i) => ({ qid: `q${i}`, question: `第 ${i + 1} 題：${long.slice(0, 80)}`, choices: ["選項甲：比較便宜但出貨慢", "選項乙：比較貴但隔天到貨", "選項丙：先不要買"], multi_select: false }));
  fixture.state.activity = { state: "waiting_input", running: true, queued: false, observed_at_ms: 2, active_task_ids: ["t1"],
    question: { method: "clarify", kind: "hermes", question_id: "long", prompt: long, hermes_questions: questions } };
  await emit(page, [{ kind: "runtime", task_id: "t1", payload: { type: "question" } }]);
  const card = page.getByRole("form", { name: "Bot 的問題" });
  await expect(card).toBeVisible();
  const box = await card.boundingBox(); const viewport = page.viewportSize()!;
  expect(box!.height).toBeLessThan(viewport.height * .65);
  expect(await card.evaluate(el => el.scrollHeight > el.clientHeight && getComputedStyle(el).overflowY === "auto")).toBe(true);
  const last = card.getByRole("radio", { name: "選項丙：先不要買" }).last();
  await last.scrollIntoViewIfNeeded(); await last.click();
  await expect(last).toBeChecked();
  for (const button of await card.locator(".question-actions button").all()) await expect(button).toBeInViewport();
});

test("clarify, approval and secret questions are answered from the docked card", async ({ page }) => {
  const fixture = setup();
  await open(page, fixture);
  const waiting = (question: Json) => ({ state: question.method === "approval" ? "waiting_approval" : "waiting_input", running: true, queued: false, observed_at_ms: 2, active_task_ids: ["t1"], question });

  fixture.state.activity = waiting({ method: "clarify", kind: "hermes", question_id: "q1", hermes_questions: [{ qid: "a", question: "要哪一種格式？", choices: ["表格", "條列"], multi_select: false }] });
  await emit(page, [{ kind: "runtime", task_id: "t1", payload: { type: "question" } }]);
  const clarify = page.getByRole("form", { name: "Bot 的問題" });
  await expect(clarify).toContainText("要哪一種格式？");
  await expect(page.locator(".chat-status")).toContainText("等你回覆");
  fixture.state.activity = { ...IDLE, state: "running", running: true, active_task_ids: ["t1"] };
  await clarify.getByRole("radio", { name: "條列" }).click();
  await expect.poll(() => fixture.state.posts.find(post => post.path.endsWith("/questions"))?.body).toEqual({ question_id: "q1", answers: { a: "條列" } });
  await expect(clarify).toHaveCount(0);

  fixture.state.activity = waiting({ method: "approval", question_id: "q2", prompt: "rm -rf /workspace/tmp" });
  await emit(page, [{ kind: "runtime", task_id: "t1", payload: { type: "question" } }]);
  const approval = page.getByRole("region", { name: "需要你的允許" });
  await expect(approval).toContainText("rm -rf /workspace/tmp");
  await expect(page.getByRole("textbox", { name: "訊息" })).toBeDisabled();
  fixture.state.activity = { ...IDLE, state: "running", running: true, active_task_ids: ["t1"] };
  await approval.getByRole("button", { name: "允許這次" }).click();
  await expect.poll(() => fixture.state.posts.filter(post => post.path.endsWith("/questions")).at(-1)?.body).toEqual({ question_id: "q2", choice: "once" });

  fixture.state.activity = waiting({ kind: "secret", question_id: "q3", prompt: "請提供網站密碼", fields: [{ name: "value", label: "密碼", kind: "password" }] });
  await emit(page, [{ kind: "runtime", task_id: "t1", payload: { type: "question" } }]);
  const secret = page.getByRole("form", { name: "需要秘密輸入" });
  await secret.getByLabel("密碼").fill("s3cret-value");
  fixture.state.activity = { ...IDLE };
  await secret.getByRole("button", { name: "送出" }).click();
  await expect.poll(() => fixture.state.posts.find(post => post.path.endsWith("/secrets"))?.body).toMatchObject({ question_id: "q3", value: "s3cret-value" });
  await expect(secret).toHaveCount(0);
  await expect(page.getByRole("textbox", { name: "訊息" })).toBeEnabled();
});

test("panel, composer drafts and keyboard behave like a chat app", async ({ page }, info) => {
  const fixture = setup();
  await open(page, fixture);
  const composer = page.getByRole("textbox", { name: "訊息" });
  await composer.fill("第一行");
  await composer.press("Shift+Enter");
  await composer.pressSequentially("第二行");
  await expect(composer).toHaveValue("第一行\n第二行");
  await page.getByRole("button", { name: "電腦", exact: true }).click();
  const panel = page.getByRole("complementary", { name: "電腦" });
  await expect(panel).toContainText("測試環境沒有電腦");
  if (info.project.name === "mobile") {
    await page.goBack();
    await expect(panel).toHaveCount(0);
    await expect(page.locator(".chat-title")).toHaveText("測試 Bot");
  } else {
    await page.getByRole("button", { name: "收起面板" }).click();
    await expect(panel).toHaveCount(0);
  }
  await expect(composer).toHaveValue("第一行\n第二行");
  await composer.press("Enter");
  await expect.poll(() => fixture.state.posts.find(post => post.path.endsWith("/messages"))?.body.text).toBe("第一行\n第二行");
  await expect(page.locator(".msg.user").last()).toContainText("第二行");
});

const RICH_REPLY = [
  "## 本週重點整理",
  "**「營收」**比上週成長 12%，主要來自**新客戶**。",
  "第一行說明",
  "第二行說明",
  "",
  "1. 先確認報表",
  "   - 核對 *合計*",
  "   - 標出 ~~舊數字~~",
  "2. 再寄給團隊",
  "",
  "- [x] 已下載資料",
  "- [ ] 等待主管確認",
  "",
  "> 提醒：週五前要交。",
  "",
  "| 項目 | 數量 | 狀態 |",
  "| --- | ---: | --- |",
  "| 訂單 | 128 | 完成 |",
  "| 退貨 | 3 | 處理中 |",
  "",
  "執行 `npm run build` 後再部署：",
  "",
  "```python",
  "def total(rows):",
  "    return sum(row[\"amount\"] for row in rows)",
  "```",
  "",
  "---",
  "詳情見 https://example.com/report",
].join("\n");

test("assistant replies render chat-friendly markdown", async ({ page, context }, info) => {
  const fixture = setup();
  fixture.state.transcript.push({ id: "delta:t9", role: "assistant", content: RICH_REPLY, at: "2026-10-08T01:02:00Z" });
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await open(page, fixture);
  const reply = page.locator(".msg.assistant").last();
  await expect(reply.locator("h2")).toHaveText("本週重點整理");
  await page.screenshot({ path: info.outputPath("markdown.png"), fullPage: false });
  await expect(reply.locator("strong").first()).toHaveText("「營收」");
  await expect(reply.locator("strong").nth(1)).toHaveText("新客戶");
  await expect(reply.locator("p", { hasText: "第一行說明" }).locator("br")).toHaveCount(2);
  await expect(reply.locator("del")).toHaveText("舊數字");
  await expect(reply.locator("li.task-list-item input[type=checkbox]")).toHaveCount(2);
  await expect(reply.locator("td").nth(1)).toHaveCSS("text-align", "right");
  const code = reply.locator(".md-code");
  await expect(code.locator(".md-code-lang")).toHaveText("python");
  await expect(code.locator(".hljs-keyword").first()).toHaveText("def");
  await code.getByRole("button", { name: "複製程式碼" }).click();
  await expect(code.getByRole("button", { name: "已複製" })).toBeVisible();
  if (info.project.name === "desktop") expect(await page.evaluate(() => navigator.clipboard.readText())).toContain("def total(rows):");
  await expect(reply.getByRole("link", { name: "example.com" })).toHaveAttribute("href", "https://example.com/report");
  await reply.scrollIntoViewIfNeeded();
  await reply.screenshot({ path: info.outputPath("markdown-reply.png") });
});

test("file paths written into Chinese sentences open the right file, folder, or teammate's copy", async ({ page }) => {
  const fixture = setup();
  fixture.state.transcript.push({ id: "delta:t10", role: "assistant", at: "2026-10-08T01:03:00Z",
    content: "已寫入 /workspace/reports/log.md，等你確認。腳本 /workspace/poc/run.py（純標準庫）、程式都在 /workspace/poc/ 裡；伺服器是 /workspace/poc/server.py。" });
  await open(page, fixture);
  const reply = page.locator(".msg.assistant").last();
  await expect(reply).toContainText("，等你確認。");
  await expect(reply).toContainText("（純標準庫）、程式都在");
  // Each name is the file itself, without the sentence that followed it.
  for (const name of ["log.md", "run.py", "poc", "server.py"]) await expect(reply.getByRole("button", { name: `開啟 ${name}`, exact: true })).toBeVisible();
  await reply.getByRole("button", { name: "開啟 log.md", exact: true }).click();
  const viewer = page.locator(".file-viewer");
  await expect(viewer.locator(".file-viewer-head strong")).toHaveText("log.md");
  await expect(viewer).toContainText("FIXTURE_REPORT");
  // On a phone the files panel stays open over the chat; go back to the conversation.
  const close = async () => {
    await viewer.getByRole("button", { name: /關閉|Close|×/ }).click();
    const back = page.getByRole("button", { name: "返回對話" });
    if (await back.isVisible()) await back.click();
  };
  await close();
  // A folder lists what is in it; a teammate's file says whose computer it is on.
  await reply.getByRole("button", { name: "開啟 poc", exact: true }).click();
  await expect(viewer).toContainText("這是資料夾");
  await viewer.getByRole("button", { name: "log.md" }).click();
  await expect(viewer.locator(".file-viewer-head strong")).toHaveText("log.md");
  await close();
  await reply.getByRole("button", { name: "開啟 server.py", exact: true }).click();
  await expect(viewer).toContainText("它在 技術主管 的電腦上");
  await expect(viewer.getByRole("button", { name: "打開 技術主管 那份" })).toBeVisible();
});

test("a web address followed by Chinese punctuation keeps the sentence after it", async ({ page }) => {
  const fixture = setup();
  const text = "請看 https://zh.wikipedia.org/wiki/咖啡），整理三種咖啡豆的差異。另見 www.example.com/a，謝謝";
  fixture.state.transcript.push(
    { id: "u9", role: "user", content: text, at: "2026-10-08T01:02:00Z" },
    { id: "delta:t9", role: "assistant", content: text, at: "2026-10-08T01:03:00Z" },
  );
  await open(page, fixture);
  for (const message of [page.locator(".msg.user").last(), page.locator(".msg.assistant").last()]) {
    // Markdown renders after its engine loads; the raw text shown before that must not satisfy the checks.
    const wiki = message.getByRole("link", { name: "zh.wikipedia.org" });
    await expect(wiki).toBeVisible();
    expect(decodeURI(await wiki.getAttribute("href") ?? "")).toBe("https://zh.wikipedia.org/wiki/咖啡");
    await expect(message.getByRole("link", { name: "www.example.com/a" })).toHaveAttribute("href", "http://www.example.com/a");
    await expect(message).toContainText("），整理三種咖啡豆的差異。另見");
    await expect(message).toContainText("，謝謝");
  }
});

test("threads split one bot into separate conversations that share its computer", async ({ page }) => {
  const fixture = setup();
  await open(page, fixture);
  await expect(page.getByRole("tablist", { name: "對話串" })).toHaveCount(0);
  if ((page.viewportSize()?.width || 1280) <= 860) {
    // A phone header keeps three actions; 新對話 is in its menu.
    await page.getByRole("button", { name: "更多" }).click();
    await page.getByRole("menuitem", { name: "新對話" }).click();
  } else await page.getByRole("button", { name: "新對話" }).click();
  const tabs = page.getByRole("tablist", { name: "對話串" });
  await expect(tabs.getByRole("tab", { name: "新對話" })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".chat-empty")).toContainText("測試 Bot 的另一段對話");
  const composer = page.getByRole("textbox", { name: "訊息" });
  await composer.fill("整理十月發票");
  await composer.press("Enter");
  await expect.poll(() => fixture.state.posts.find(post => post.path.endsWith("/messages"))?.body).toMatchObject({ text: "整理十月發票", thread_id: "thread-1" });

  fixture.state.threadTranscripts["thread-1"] = [{ id: "u-side", role: "user", content: "整理十月發票", at: "2026-10-08T02:00:00Z", thread: "thread-1" }];
  fixture.state.threads[1] = { ...fixture.state.threads[1], title: "十月發票", running: true };
  fixture.state.activity = { state: "running", running: true, queued: false, observed_at_ms: 1, active_task_ids: ["t1"], thread_id: "thread-1", queued_thread_ids: [] };
  await emit(page, [{ kind: "thread_updated", payload: { thread_id: "thread-1" } }, { kind: "user_message", task_id: "t1", payload: { thread_id: "thread-1" } }]);
  await expect(tabs.getByRole("tab", { name: "十月發票" })).toBeVisible();
  await expect(page.locator(".work-card")).toBeVisible();

  await tabs.getByRole("tab", { name: "主要對話" }).click();
  await expect(page.locator(".msg.user").first()).toContainText("幫我整理今天的新聞");
  await expect(page.locator(".work-card")).toHaveCount(0);
  const elsewhere = page.locator(".elsewhere-bar");
  await expect(elsewhere).toContainText("正在「十月發票」工作");
  await expect(composer).toHaveAttribute("placeholder", /正在其他對話工作/);
  await emit(page, [{ kind: "runtime", task_id: "t1", payload: { type: "message_delta", id: "t1", delta: "SIDE_ONLY_TEXT", thread_id: "thread-1" } }]);
  await page.waitForTimeout(200);
  await expect(page.locator(".chat-content")).not.toContainText("SIDE_ONLY_TEXT");

  await elsewhere.getByText("前往").click();
  await expect(tabs.getByRole("tab", { name: "十月發票" })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".work-card")).toBeVisible();
  await expect(page.locator(".msg.user").first()).toContainText("整理十月發票");

  fixture.state.activity = { ...IDLE, thread_id: null, queued_thread_ids: [] };
  fixture.state.threads[1] = { ...fixture.state.threads[1], running: false };
  await emit(page, [{ kind: "runtime", task_id: "t1", payload: { type: "turn_ended", verdict: "answer", thread_id: "thread-1" } }]);
  await expect(page.locator(".work-card")).toHaveCount(0);
  await page.getByRole("button", { name: "「十月發票」選項" }).click();
  await page.getByRole("menuitem", { name: "重新命名" }).click();
  const rename = page.getByRole("dialog", { name: "重新命名對話" });
  await rename.getByLabel("對話名稱").fill("發票整理");
  await rename.getByRole("button", { name: "儲存" }).click();
  await expect(tabs.getByRole("tab", { name: "發票整理" })).toBeVisible();
  expect(fixture.state.requests.find(r => r.method === "PATCH")?.body).toEqual({ title: "發票整理" });

  await page.getByRole("button", { name: "「發票整理」選項" }).click();
  await page.getByRole("menuitem", { name: "刪除對話" }).click();
  await page.getByRole("dialog", { name: "刪除對話" }).getByRole("button", { name: "刪除" }).click();
  await expect(tabs).toHaveCount(0);
  expect(fixture.state.requests.some(r => r.method === "DELETE" && r.path.endsWith("/threads/thread-1"))).toBe(true);
  await expect(page.locator(".msg.user").first()).toContainText("幫我整理今天的新聞");
});

test("scheduled runs appear as a collapsible routine card, not as something the user typed", async ({ page }) => {
  const fixture = setup();
  fixture.state.transcript = [
    { id: "r1", role: "user", content: "追蹤 Threads 帳號的成長數據。\n\n步驟:\n1. 開啟洞察報告", at: "2026-10-12T01:00:00Z", routine_id: "routine-1", routine_title: "Threads 每週成長追蹤" },
    { id: "a1", role: "assistant", content: "本週淨追蹤 +12，已達標。", at: "2026-10-12T01:05:00Z" },
  ];
  await open(page, fixture);
  const card = page.locator(".bubble.routine");
  const toggle = card.getByRole("button", { name: "定時任務：Threads 每週成長追蹤" });
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(card).not.toContainText("開啟洞察報告");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(card).toContainText("開啟洞察報告");
});

test("background work shows on the reply that started it, can be stopped, and its report is labelled", async ({ page }) => {
  const fixture = setup();
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  const started = new Date(Date.now() - 65_000).toISOString();
  const workers = [
    { id: "d:0:a", goal: "調查 Threads 熱門話題", status: "running", background: true, started_at: started, at: new Date().toISOString(), tool: "web_extract", detail: "https://example.com/%E8%9D%A6%E7%9A%AE" },
    { id: "d:1:b", goal: "蝦皮分潤規則", status: "completed", background: true, started_at: started, duration: 44.8 },
  ];
  fixture.state.tasks.t0 = { ...fixture.state.tasks.t0, background: workers };
  fixture.state.activity = { ...IDLE, background: 1, background_task_ids: ["t0"] };
  await open(page, fixture);
  const card = page.getByRole("region", { name: "背景工作" });
  await expect(card).toContainText("背景還有 1 項工作在跑");
  await expect(card).toContainText("已 1 分");
  await expect(card.locator("li[data-status=running]")).toContainText("讀取網頁");
  await expect(card.locator("li[data-status=running] .worker-note")).toHaveText("https://example.com/蝦皮");
  await expect(card.locator("li[data-status=completed]")).toContainText("45 秒");
  await expect(page.locator(".chat-status")).toContainText("背景工作中（1）");
  // The composer stays free: background work never blocks a new message.
  await expect(page.getByRole("textbox", { name: "訊息" })).not.toHaveAttribute("placeholder", /補充說明/);

  await card.getByRole("button", { name: "全部停止" }).click();
  await expect.poll(() => fixture.state.posts.some(post => post.path.endsWith("/stop"))).toBe(true);
  const stopped = workers.map(worker => worker.status === "running" ? { ...worker, status: "interrupted", summary: "Operation interrupted" } : worker);
  fixture.state.activity = { ...IDLE };
  await emit(page, [{ kind: "runtime", task_id: "t0", payload: { type: "workers", workers: stopped } }]);
  await expect(card).toHaveCount(0);
  const done = page.locator("details.workers-card");
  await expect(done.locator("summary")).toHaveText(/背景工作已結束 · 完成 1 · 未完成 1/);
  await done.locator("summary").click();
  await expect(done.locator("li[data-status=interrupted]")).toContainText("已停止");
  await expect(done.locator("li[data-status=interrupted] .worker-note")).toHaveCount(0);
  await expect(page.locator(".chat-status")).toContainText("待命");

  fixture.state.tasks.t9 = { id: "t9", kind: "background", state: "terminal", goal: "背景工作完成，回報結果", verdict: "answer", artifacts: [] };
  fixture.state.transcript = [...fixture.state.transcript, { id: "delta:t9", role: "assistant", content: "兩組研究都回來了。", at: "2026-10-08T01:09:00Z" }];
  await emit(page, [{ kind: "reply", task_id: "t9", payload: {} }]);
  await expect(page.locator(".msg.assistant", { hasText: "兩組研究都回來了" }).locator(".background-origin")).toHaveText("背景工作回報");
  expect(errors).toEqual([]);
});

test("teaching: demonstrate on the bot's computer, edit the steps, and hand it over to learn", async ({ page }) => {
  const fixture = setup();
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await open(page, fixture);
  await page.getByRole("button", { name: "加入檔案或教它一件事" }).click();
  await page.getByRole("button", { name: "教它一件事", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "教它一件事" });
  await expect(dialog).toContainText("密碼欄位只會記成 ••••");
  await dialog.getByLabel("你要教它做什麼？（選填）").fill("查訂單");
  await dialog.getByRole("button", { name: "開始示範" }).click();
  await expect(dialog).toHaveCount(0);
  expect(fixture.state.posts.find(post => post.path.endsWith("/teach"))?.body).toMatchObject({ action: "start", goal: "查訂單", trial: true, thread: AGENT });

  const bar = page.getByRole("region", { name: "教學" });
  await expect(bar).toContainText("教學中 · 第 0 步");
  await expect(bar.getByRole("button", { name: "完成，請它學起來" })).toBeDisabled();
  fixture.state.activity = { ...IDLE, teaching: "L1" };
  const steps = [
    { id: "b1", at: 1, source: "browser", kind: "click", text: "點擊連結「訂單」", shot: "shots/b-1.jpg", target: { role: "link", name: "訂單", box: [10, 10, 40, 20], viewport: [1280, 600] } },
    { id: "b2", at: 2, source: "browser", kind: "click", text: "點擊按鈕「點錯了」" },
  ];
  await emit(page, [{ kind: "teaching", payload: { lesson: { ...fixture.state.lesson, steps } } }]);
  fixture.state.lesson = { ...fixture.state.lesson, steps };
  await expect(bar).toContainText("第 2 步");
  await expect(bar).toContainText("最新：點擊按鈕「點錯了」");
  await expect(page.locator(".chat-status")).toContainText("教學中");

  await bar.getByRole("button", { name: /步驟/ }).click();
  await expect(bar.locator(".teach-steps li")).toHaveCount(2);
  await expect(bar.locator(".teach-steps li").first().locator(".teach-mark")).toHaveCount(1);
  await bar.getByRole("button", { name: "刪除這一步：點擊按鈕「點錯了」" }).click();
  await expect(bar.locator(".teach-steps li")).toHaveCount(1);
  await bar.getByRole("button", { name: "補充說明" }).click();
  await bar.getByLabel("補充說明").fill("只看已完成的訂單");
  await bar.getByRole("button", { name: "加入" }).click();
  await expect(bar.locator(".teach-steps li.is-note")).toContainText("只看已完成的訂單");
  await bar.getByRole("button", { name: "暫停" }).click();
  await expect(bar).toContainText("已暫停");
  await bar.getByRole("button", { name: "繼續" }).click();
  await expect(bar).toContainText("教學中");

  await bar.getByRole("button", { name: "完成，請它學起來" }).click();
  await expect(bar).toHaveCount(0);
  const demo = page.locator(".msg.user .demo-card");
  await expect(demo.locator("summary")).toContainText("我示範了「查訂單」，共 2 步");
  await demo.locator("summary").click();
  await expect(demo.locator(".teach-steps li")).toHaveCount(2);
  await expect(demo.getByRole("button", { name: /刪除這一步/ })).toHaveCount(0);
  const actions = fixture.state.posts.filter(post => post.path.endsWith("/teach")).map(post => post.body.action);
  expect(actions).toEqual(["start", "drop", "note", "pause", "resume", "finish"]);
  expect(errors).toEqual([]);
});

test("a reply loaded mid-stream does not repeat text the reconnecting stream replays", async ({ page }) => {
  const fixture = setup();
  fixture.state.activity = { ...IDLE, state: "running", running: true, active_task_ids: ["t1"] };
  fixture.state.transcript = [...fixture.state.transcript,
    { id: "m2", role: "user", content: "寫一份報告", at: "2026-10-08T01:02:00Z" },
    { id: "delta:t1", role: "assistant", content: "第一段", at: "2026-10-08T01:02:10Z", streaming: true, stream_event: 50 }];
  await open(page, fixture);
  const reply = page.locator(".msg.assistant.streaming");
  await expect(reply).toContainText("第一段");
  // Event 49 is already in the loaded copy; 51 is new.
  await emit(page, [
    { id: 49, kind: "runtime", task_id: "t1", payload: { type: "message_delta", id: "t1", delta: "第一段" } },
    { id: 51, kind: "runtime", task_id: "t1", payload: { type: "message_delta", id: "t1", delta: "，第二段" } },
  ] as never);
  await expect(reply).toContainText("第一段，第二段");
  expect(((await reply.innerText()).match(/第一段/g) || []).length).toBe(1);
});

test("text shared from another app goes into the chosen conversation's composer", async ({ page }) => {
  const fixture = setup();
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.addInitScript(() => { (window as unknown as { EventSource: unknown }).EventSource = class { close() {} }; });
  await page.route("**/api/**", fixture.handle);
  await page.goto("/?share_title=" + encodeURIComponent("好文") + "&share_text=" + encodeURIComponent("看這篇 https://example.com/a") + "&share_url=" + encodeURIComponent("https://example.com/a"));
  const dialog = page.getByRole("dialog", { name: "分享到 LazyBoy" });
  await expect(dialog).toContainText("看這篇 https://example.com/a");
  await dialog.getByRole("button", { name: /測試 Bot/ }).click();
  await expect(page.getByRole("textbox", { name: "訊息" })).toHaveValue("好文\n看這篇 https://example.com/a");
  expect(new URL(page.url()).search).toBe("");
});

test("shared text reaches the side thread a bot reopens on, and a group chat that is already open", async ({ page }) => {
  const fixture = setup();
  fixture.state.threads.push({ id: "thread-1", title: "旁支", main: false, preview: "", message_count: 0, running: false, queued: false });
  const member = { id: AGENT, name: "測試 Bot", avatar_color: "#9159FE", avatar_shape: "round", seat: "idle" };
  const room = { id: "room-1", name: "小組", member_ids: [AGENT, "b2"], members: [member, { ...member, id: "b2", name: "第二 Bot" }], preview: "", working: [], message_count: 0 };
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.addInitScript(() => {
    (window as unknown as { EventSource: unknown }).EventSource = class { close() {} };
    if (!sessionStorage.getItem("seeded")) {
      sessionStorage.setItem("seeded", "1");
      localStorage.setItem("lazyhermes.active", "fixture-bot");
      localStorage.setItem("lazyhermes.threads", JSON.stringify({ "fixture-bot": "thread-1" }));
    }
  });
  await page.route("**/api/**", fixture.handle);
  await page.route("**/api/rooms**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/rooms") return route.fulfill({ json: { rooms: [room] } });
    if (path === "/api/rooms/room-1") return route.fulfill({ json: { ...room, transcript: [] } });
    return route.fulfill({ json: {} });
  });
  const share = (text: string) => page.goto("/?share_text=" + encodeURIComponent(text));
  await share("給旁支的文字");
  await page.getByRole("dialog", { name: "分享到 LazyBoy" }).getByRole("button", { name: /測試 Bot/ }).click();
  await expect(page.locator(".chat-title")).toHaveText("測試 Bot");
  await expect(page.getByRole("textbox", { name: "訊息" })).toHaveValue("給旁支的文字");

  // The group chat is on screen behind the share dialog when the text is sent to it.
  await page.evaluate(() => localStorage.setItem("lazyhermes.room", "room-1"));
  await share("給群組的文字");
  await expect(page.getByRole("textbox", { name: "群組訊息" })).toBeAttached();
  await page.getByRole("dialog", { name: "分享到 LazyBoy" }).getByRole("button", { name: /小組/ }).click();
  await expect(page.getByRole("textbox", { name: "群組訊息" })).toHaveValue("給群組的文字");
});

test("a conversation that fails to load shows the error, not the empty-chat welcome", async ({ page }) => {
  const fixture = setup();
  await open(page, fixture);
  const fail = (route: Route) => route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: "讀取失敗 FIXTURE" }) });
  await page.route(`**/api/agents/${AGENT}`, fail);
  await page.route(`**/api/agents/${AGENT}?*`, fail);
  await page.reload();
  if ((page.viewportSize()?.width || 1280) <= 860) await page.locator(".bot-row-main", { hasText: "測試 Bot" }).click();
  await expect(page.locator(".chat .pane-error")).toBeVisible();
  await expect(page.locator(".chat .chat-empty")).toHaveCount(0);
});
