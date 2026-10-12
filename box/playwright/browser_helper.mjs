#!/usr/bin/env node
/**
 * LazyBoy Playwright DOM helper (optional).
 *
 * Modes:
 *   JSONL daemon (default when no --cmd): read JSON lines from stdin, write JSON lines to stdout.
 *   One-shot:  node browser_helper.mjs --cmd '<json>'
 *   Self-test: node browser_helper.mjs --self-test   (no Chromium required)
 *
 * Ops: ping, fetch, navigate, snapshot, click, type, eval, close, status, handoff_prepare
 * Prefer CSS selector or role+name. Screenshots are NOT the primary control surface.
 */

import { randomUUID } from "node:crypto";
import { createInterface } from "node:readline";
import { dirname, resolve, relative, isAbsolute } from "node:path";
import { mkdir, stat, realpath, lstat, readFile, chmod } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const INSTALL_HINT =
  "Playwright not available. From the LazyBoy repo run: " +
  "cd tools/playwright && npm install && npx playwright install chromium";

const remoteCdp = process.env.LAZYBOY_BROWSER_CDP;
let browser = null;
let context = null;
let page = null;
let lastUrl = null;
/** Whether the current browser was launched headed (visible). */
let browserHeaded = false;
let nextTab = 0;
const tabs = new Map();
const otpTargets = new Map();
const popupParents = new WeakMap();
const snapshotStates = new WeakMap();
const actionHandles = new Set();
const snapshotKey = `__lazyboy_snapshot_${Math.random().toString(36).slice(2)}`;
let snapshotGeneration = 0;
const snapshotTarget = p => typeof p.mainFrame === "function" ? p.mainFrame() : p;
function invalidateSnapshot(p) { snapshotStates.delete(snapshotTarget(p)); }

let handoffStorage;
function registerTab(p) {
  if ([...tabs.values()].includes(p)) return;
  tabs.set(String(++nextTab), p);
  p.on("popup", popup => { popupParents.set(popup, p); registerTab(popup); });
  p.on("framenavigated", frame => invalidateSnapshot(frame));
  p.on("close", () => { invalidateSnapshot(p); for (const [id,t] of tabs) if(t===p) tabs.delete(id); if(page===p) page=[...tabs.values()][0] || null; });
}
async function target(req) {
  await ensurePage();
  if (req.tab_id) { const tab=tabs.get(String(req.tab_id)); if(!tab || tab.isClosed()) throw new Error("unknown tab_id; list tabs again"); page=tab; }
  if(req.frame) { const el=await page.locator(String(req.frame)).elementHandle(); const frame=await el?.contentFrame(); if(!frame) throw new Error("frame not found; obtain a new snapshot"); return frame; }
  return page;
}

function envHeadedDefault() {
  const v = (process.env.LAZYBOY_BROWSER_HEADED || "").trim().toLowerCase();
  if (v === "1" || v === "true" || v === "yes") return true;
  if (v === "0" || v === "false" || v === "no") return false;
  return false;
}

function wantHeaded(forceHeaded) {
  if (forceHeaded === true) return true;
  return envHeadedDefault();
}

function ok(id, extra = {}) {
  return { id: id ?? null, ok: true, ...extra };
}

function fail(id, error, extra = {}) {
  return {
    id: id ?? null,
    ok: false,
    blocked: true,
    error: String(error),
    ...extra,
  };
}

async function loadPlaywright() {
  try {
    return await import("playwright");
  } catch (e) {
    const err = new Error(`${INSTALL_HINT} (import failed: ${e.message})`);
    err.code = "PLAYWRIGHT_MISSING";
    throw err;
  }
}

async function launchBrowser(headed) {
  const { chromium } = await loadPlaywright();
  try {
    if (remoteCdp) {
      browser = await chromium.connectOverCDP(remoteCdp);
      context = browser.contexts()[0];
      if (!context) throw new Error("Box Chromium has no persistent context");
      headed = true;
    } else if (process.env.LAZYBOY_BROWSER_PROFILE) {
      await mkdir(process.env.LAZYBOY_BROWSER_PROFILE,{recursive:true,mode:0o700});
      context=await chromium.launchPersistentContext(process.env.LAZYBOY_BROWSER_PROFILE, { headless:!headed, acceptDownloads:true });
      // Chromium does not restore session cookies on a clean restart; retain them per agent session.
      const stored=await readFile(resolve(process.env.LAZYBOY_BROWSER_PROFILE,"lazyboy-state.json"),"utf8").then(JSON.parse).catch(()=>null);
      if(stored?.cookies?.length) await context.addCookies(stored.cookies);
      browser=context.browser();
    } else {
      browser=await chromium.launch({headless:!headed});
      context=await browser.newContext({acceptDownloads:true, ...(handoffStorage ? {storageState:handoffStorage} : {})});
    }
    browserHeaded = headed;
  } catch (e) {
    const err = new Error(
      `${INSTALL_HINT} (chromium launch failed: ${e.message})`
    );
    err.code = "CHROMIUM_MISSING";
    throw err;
  }
  tabs.clear();
  context.on("page",registerTab);
  for (const p of context.pages()) registerTab(p);
  page = context.pages()[0] || await context.newPage();
  registerTab(page);
  return page;
}

async function ensurePage(opts = {}) {
  const headed = wantHeaded(opts.headed === true);
  if (page) {
    // Already open; if caller needs headed and we are headless, relaunch below via handoff_prepare.
    return page;
  }
  return launchBrowser(headed);
}

/** Close current browser and reopen headed, restoring lastUrl when possible. */
async function ensureHeadedForHandoff() {
  const restore = (page && !page.isClosed() ? page.url() : null) || lastUrl;
  if (browser && browserHeaded && page) {
    try {
      await page.bringToFront();
    } catch {
      /* ignore */
    }
    return {
      prepared: true,
      headed: true,
      relaunched: false,
      url: page.url(),
      restored: false,
    };
  }

  const previousTabs = context ? await Promise.all(context.pages().filter(p=>!p.isClosed()).map(async p=>({
    url:p.url(), active:p===page,
    storage:await p.evaluate(()=>({origin:location.origin,values:Object.fromEntries(Object.entries(sessionStorage))})).catch(()=>null),
  }))) : [];
  if (context) {
    handoffStorage=await context.storageState().catch(()=>undefined);
    await persistContext();
    if (!remoteCdp) await context.close().catch(() => {});
    if (browser) await browser.close().catch(() => {});
    browser = null;
    context = null;
    page = null;
    browserHeaded = false;
  }

  await launchBrowser(true);
  let restored = false;
  if (previousTabs.length) {
    const failures=[];
    let activePage=page;
    for (const [index,tab] of previousTabs.entries()) {
      const reopened=index===0 ? page : await context.newPage();
      const cdp=tab.storage ? await context.newCDPSession(reopened) : null;
      let restoreScript;
      try {
        if(cdp) await cdp.send("Page.enable");
        if(cdp) restoreScript=await cdp.send("Page.addScriptToEvaluateOnNewDocument",{source:
          `(({origin,values})=>{if(location.origin===origin)for(const [key,value] of Object.entries(values))sessionStorage.setItem(key,value);})(${JSON.stringify(tab.storage)})`});
        if(tab.url && tab.url!=="about:blank") {
          await reopened.goto(tab.url,{waitUntil:"domcontentloaded",timeout:30000}).catch(e=>failures.push(String(e.message)));
        }
      } finally {
        if(restoreScript) await cdp.send("Page.removeScriptToEvaluateOnNewDocument",{identifier:restoreScript.identifier});
        if(cdp) await cdp.detach();
      }
      if(tab.active) activePage=reopened;
    }
    page=activePage;
    lastUrl=page.url();
    await page.bringToFront().catch(()=>{});
    return {prepared:true,headed:true,relaunched:true,url:lastUrl,restored:failures.length===0,
      tabs_restored:previousTabs.length,...(failures.length ? {restore_errors:failures} : {})};
  }
  if (restore && restore !== "about:blank") {
    try {
      await page.goto(String(restore), {
        waitUntil: "domcontentloaded",
        timeout: 30000,
      });
      lastUrl = page.url();
      restored = true;
    } catch (e) {
      // Keep headed page even if restore fails; caller still sees visible window.
      lastUrl = page.url();
      return {
        prepared: true,
        headed: true,
        relaunched: true,
        url: lastUrl,
        restored: false,
        restore_error: String(e.message || e),
      };
    }
  }
  try {
    await page.bringToFront();
  } catch {
    /* ignore */
  }
  return {
    prepared: true,
    headed: true,
    relaunched: true,
    url: page ? page.url() : null,
    restored,
  };
}

/** Accessibility-ish text snapshot: roles, names, and useful selectors — not pixels. */
async function buildSnapshot(p) {
  const snapshotId = ++snapshotGeneration;
  const nodes = await p.evaluate(({key, generation}) => {
    const refs = new Map();
    Object.defineProperty(globalThis, key, {value: {generation, refs}, configurable: true});
    const out = [];
    const max = 200;
    const interesting = new Set([
      "a",
      "button",
      "input",
      "textarea",
      "select",
      "option",
      "h1",
      "h2",
      "h3",
      "h4",
      "h5",
      "h6",
      "label",
      "li",
      "summary",
      "nav",
      "main",
      "form",
      "img",
      "table",
      "th",
      "td",
    ]);

    function cssPath(el) {
      if (el.id) return `#${CSS.escape(el.id)}`;
      const parts = [];
      let cur = el;
      while (cur && cur.nodeType === 1 && parts.length < 5) {
        let part = cur.tagName.toLowerCase();
        if (cur.id) {
          parts.unshift(`#${CSS.escape(cur.id)}`);
          break;
        }
        const parent = cur.parentElement;
        if (parent) {
          const siblings = [...parent.children].filter(
            (c) => c.tagName === cur.tagName
          );
          if (siblings.length > 1) {
            const idx = siblings.indexOf(cur) + 1;
            part += `:nth-of-type(${idx})`;
          }
        }
        parts.unshift(part);
        cur = parent;
      }
      return parts.join(" > ");
    }

    const walker = document.createTreeWalker(
      document.body || document.documentElement,
      NodeFilter.SHOW_ELEMENT
    );
    let node = walker.currentNode;
    while (node && out.length < max) {
      const tag = node.tagName ? node.tagName.toLowerCase() : "";
      const role =
        node.getAttribute("role") ||
        (tag === "a"
          ? "link"
          : tag === "button"
            ? "button"
            : tag === "input"
              ? node.getAttribute("type") === "submit"
                ? "button"
                : "textbox"
              : tag === "textarea"
                ? "textbox"
                : tag === "img"
                  ? "img"
                  : tag.startsWith("h") && tag.length === 2
                    ? "heading"
                    : null);
      const name =
        node.getAttribute("aria-label") ||
        node.getAttribute("alt") ||
        node.getAttribute("placeholder") ||
        node.getAttribute("title") ||
        (node.innerText || node.textContent || "").trim().slice(0, 120);
      const href = node.getAttribute && node.getAttribute("href");
      const type = node.getAttribute && node.getAttribute("type");
      const value =
        type !== "password" && !node.__lazyboySensitive && node.getAttribute("autocomplete") !== "one-time-code" && "value" in node && typeof node.value === "string"
          ? String(node.value).slice(0, 80)
          : null;

      if (interesting.has(tag) || node.getAttribute("role") || node.id) {
        const entry = {
          ref: `${key}-${generation}-${out.length}`,
          tag,
          role: role || tag,
          name: name || "",
          selector: cssPath(node),
        };
        if (href) entry.href = href;
        if (type) entry.type = type;
        if (value) entry.value = value;
        if (node.id) entry.id = node.id;
        refs.set(entry.ref, node);
        out.push(entry);
      }
      node = walker.nextNode();
    }
    return out;
  }, {key: snapshotKey, generation: snapshotId});
  snapshotStates.set(snapshotTarget(p), snapshotId);

  const title = await p.evaluate(() => document.title);
  const url = p.url();
  const lines = nodes.map((n, i) => {
    const bits = [`[${i}]`, `ref=${n.ref}`, n.role || n.tag];
    if (n.name) bits.push(`name=${JSON.stringify(n.name)}`);
    if (n.selector) bits.push(`sel=${n.selector}`);
    if (n.href) bits.push(`href=${n.href}`);
    if (n.type) bits.push(`type=${n.type}`);
    if (n.value) bits.push(`value=${JSON.stringify(n.value)}`);
    return bits.join(" ");
  });

  return {
    url,
    title,
    snapshot_id: snapshotId,
    count: nodes.length,
    text: lines.join("\n"),
    nodes,
    frames: await p.locator("iframe").evaluateAll(els=>els.map((el,i)=>({selector:el.id ? `#${CSS.escape(el.id)}` : `iframe:nth-of-type(${i+1})`,src:el.src,name:el.name}))),
  };
}

async function resolveLocator(p, args) {
  if (args.ref) {
    if (!Number.isInteger(args.snapshot_id) || snapshotStates.get(snapshotTarget(p)) !== args.snapshot_id) {
      throw new Error("stale_snapshot: take a fresh browser_snapshot and use its ref and snapshot_id");
    }
    const handle = await p.evaluateHandle(({key, generation, ref}) => {
      const state = globalThis[key];
      const el = state?.generation === generation ? state.refs.get(ref) : null;
      return el?.isConnected ? el : null;
    }, {key: snapshotKey, generation: args.snapshot_id, ref: String(args.ref)});
    const element = handle.asElement();
    if (!element) { await handle.dispose(); throw new Error("stale_ref: element was removed; take a fresh browser_snapshot"); }
    actionHandles.add(handle);
    return element;
  }
  let locator;
  if (args.selector) locator = p.locator(String(args.selector));
  else if (args.role) locator = p.getByRole(String(args.role), args.name ? {name: String(args.name), exact: true} : {});
  else if (args.text) locator = p.getByText(String(args.text), {exact: !!args.exact});
  else if (args.label) locator = p.getByLabel(String(args.label), {exact: true});
  else if (args.placeholder) locator = p.getByPlaceholder(String(args.placeholder), {exact: true});
  else throw new Error("action requires ref+snapshot_id, selector, role(+name), text, label, or placeholder");
  if (await locator.count() > 1) throw new Error("ambiguous_target: multiple elements match; take a snapshot and choose an exact ref");
  return locator;
}

// Observation failure must never hide an action that already happened or invite a replay.
async function actionResult(req, p, result) {
  if (req.observe_after === true && !req.secret_name && process.env.LAZYBOY_OBSERVE_AFTER !== "0") {
    try {
      const snap = await buildSnapshot(p);
      result.observation = {url: snap.url, title: snap.title, snapshot_id: snap.snapshot_id,
        body_text: (await p.locator("body").innerText({timeout:500})).slice(0,1500),
        tab_id: [...tabs].find(([,tab]) => tab === (typeof p.page === "function" ? p.page() : p))?.[0] ?? null,
        snapshot: snap.text.slice(0, 6000), truncated: snap.text.length > 6000};
    } catch (error) { result.observation_error = String(error.message || error); }
  }
  return ok(req.id, result);
}

// Fast path only for unambiguous, standard OTP forms. Unknown widgets stay with
// the model/human; no brand-specific selectors or guessed submit buttons.
async function detectChallenge(p) {
  const selector = "input[autocomplete='one-time-code']";
  const inputs = await p.locator(selector).elementHandles();
  if (!inputs.length) {
    const frames = await p.locator("iframe[title]").elementHandles();
    for (const frame of frames) {
      const eligible = await frame.evaluate(el => {
        const r = el.getBoundingClientRect();
        return r.width >= 100 && r.height >= 60 && !el.__lazyboyChallengeOffered
          && /captcha|verification challenge|驗證|验证/i.test(el.title);
      });
      if (eligible && await frame.isVisible()) {
        await frame.evaluate(el => { el.__lazyboyChallengeOffered = true; });
        return {kind:"box_help",label:"請在原頁面完成圖片驗證，再交回操作權",reason:"complex_native_ui",detected_at_ms:Date.now()};
      }
    }
    return null;
  }
  if (inputs.length > 12) return null;
  for (const el of inputs) {
    if (!await el.isVisible() || !await el.isEditable() || await el.evaluate(el => !!el.__lazyboyChallengeOffered)) return null;
  }
  const eligible = await inputs[0].evaluate(el => {
    const form = el.form;
    if (!form || form.querySelector('input[type="password"]')) return false;
    return [...form.querySelectorAll('button[type="submit"],input[type="submit"],button:not([type])')]
      .filter(b => b.getClientRects().length && !b.disabled).length === 1;
  });
  if (!eligible) return null;
  const form = await inputs[0].evaluateHandle(el => el.form);
  try {
    for (const el of inputs) if (!await el.evaluate((el, form) => el.form === form, form)) return null;
  } finally { await form.dispose(); }
  const submitHandle = await inputs[0].evaluateHandle(el => [...el.form.querySelectorAll('button[type="submit"],input[type="submit"],button:not([type])')]
    .find(b => b.getClientRects().length && !b.disabled));
  const submit = submitHandle.asElement();
  if (!submit) return null;
  const lengths = await Promise.all(inputs.map(el => el.getAttribute("maxlength")));
  if (inputs.length > 1 && lengths.some(n => n !== "1")) return null;
  const token = randomUUID();
  for (const el of inputs) await el.evaluate(el => { el.__lazyboySensitive = true; el.__lazyboyChallengeOffered = true; });
  for (const [key, binding] of otpTargets) if (Date.now() - binding.created > 300000) otpTargets.delete(key);
  const digits = inputs.length > 1 ? inputs.length : null;
  otpTargets.set(token, {p, url:p.url(), inputs, submit, digits, kind:"otp_code", autoSubmit:false, created:Date.now()});
  return {kind:"otp_code", otp_token:token, digits, label:"請輸入這個頁面的驗證碼", detected_at_ms:Date.now()};
}

async function handle(req) {
  let result = await handleOperation(req);
  if (result.ok && req.op === "navigate" && req.observe_after === true) result = await actionResult(req, await target(req), result);
  if (result.ok && req.detect_challenge === true && process.env.LAZYBOY_CHALLENGE_FAST !== "0"
      && ["navigate", "click", "press", "snapshot", "dom", "wait"].includes(req.op)) {
    try {
      const p = await target(req);
      result.challenge = await detectChallenge(p);
    } catch { /* A failed observation cannot turn an executed action into a retry. */ }
  }
  return result;
}

async function handleOperation(req) {
  const id = req.id ?? null;
  const op = req.op || req.command;
  if (!op) return fail(id, "missing op");

  try {
    switch (op) {
      case "fetch": {
        const url = new URL(String(req.url));
        if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) {
          return fail(id, "session fetch requires an http/https URL without embedded credentials");
        }
        await ensurePage();
        // Auto only adopts a session for a known origin or applicable cookies.
        // Cookie values and storage never cross the helper protocol.
        if (req.session === "auto") {
          const cookies = await context.cookies([url.href]);
          const visited = [...tabs.values()].some(tab => {
            try { return new URL(tab.url()).origin === url.origin; } catch { return false; }
          });
          const saved = !cookies.length && !visited
            && (await context.storageState()).origins.some(entry => entry.origin === url.origin);
          if (!cookies.length && !visited && !saved) return ok(id, {session_selected:false});
        }
        let response;
        try {
          // The associated API context shares live cookies and applies Set-Cookie
          // back to Chromium. No navigation, DOM evaluation, or request replay.
          // Rust validates every redirect before explicitly requesting the next hop.
          response = await context.request.get(url.href, {
            maxRedirects: 0, timeout: 30000, failOnStatusCode: false,
          });
          const body = await response.body();
          const headers = response.headers();
          const limit = 2 * 1024 * 1024;
          return ok(id, {session_selected:true, url:response.url(), status:response.status(),
            content_type:headers["content-type"] || "", location:headers.location || null,
            body_base64:body.subarray(0,limit).toString("base64"), body_truncated:body.length>limit});
        } catch (error) {
          // Playwright call logs may contain request headers. Return no raw log.
          return fail(id, `browser_session_fetch_failed: ${error.name === "TimeoutError" ? "timeout" : "connection or response error"}; inspect the current session before retrying`);
        } finally {
          await response?.dispose();
        }
      }
      case "poll_challenge": {
        if (!page || page.isClosed()) return ok(id, {challenge:null});
        const changed = await page.evaluate(() => {
          if (!globalThis.__lazyboyChallengeWatch) {
            const state = {version:1, seen:0};
            new MutationObserver(() => state.version++).observe(document.documentElement,
              {subtree:true, childList:true, attributes:true, attributeFilter:["disabled","style","class","autocomplete","type","title","src"]});
            globalThis.__lazyboyChallengeWatch = state;
          }
          const state = globalThis.__lazyboyChallengeWatch;
          const changed = state.seen !== state.version;
          state.seen = state.version;
          return changed;
        });
        return ok(id, {challenge:changed ? await detectChallenge(page) : null});
      }
      case "prepare_challenge":
      case "prepare_otp": {
        const kind = op === "prepare_otp" ? "otp_code" : req.kind;
        if (!["otp_code", "captcha_image"].includes(kind)) return fail(id, "invalid_challenge_kind");
        const p = await target(req);
        const selector = req.selector || (kind === "otp_code" ? "input[autocomplete='one-time-code']" : null);
        if (!selector) return fail(id, "challenge_target_required");
        const inputs = await p.locator(selector).elementHandles();
        if (!inputs.length || inputs.length > (kind === "captcha_image" ? 1 : 12)) return fail(id, "otp_target_missing_or_ambiguous");
        for (const input of inputs) {
          if (!await input.isVisible() || !await input.isEnabled() || !await input.isEditable()) return fail(id, "otp_target_not_ready");
        }
        const lengths = await Promise.all(inputs.map(input => input.getAttribute("maxlength")));
        if (inputs.length > 1 && lengths.some(n => n !== "1")) return fail(id, "otp_target_ambiguous");
        const digits = req.digits || (inputs.length > 1 ? inputs.length : null);
        if (digits && (!Number.isInteger(digits) || digits < 1 || digits > 12 || (inputs.length > 1 && digits !== inputs.length))) return fail(id, "otp_invalid_digits");
        let submit = null;
        if (req.submit_selector) {
          const buttons = await p.locator(req.submit_selector).elementHandles();
          if (buttons.length !== 1 || !await buttons[0].isVisible()) return fail(id, "otp_submit_ambiguous");
          submit = buttons[0];
        } else if (req.auto_submit !== true && kind === "otp_code") {
          return fail(id, "otp_submit_required: provide observed submit_selector or auto_submit=true for a page that submits on input");
        }
        for (const [key, binding] of otpTargets) {
          if (binding.p === p || Date.now() - binding.created > 300000) otpTargets.delete(key);
        }
        for (const input of inputs) await input.evaluate(el => { el.__lazyboySensitive = true; });
        otpTargets.set(req.token, { p, url: p.url(), inputs, digits, submit, kind, autoSubmit: req.auto_submit === true, created: Date.now() });
        let imageBase64;
        if (kind === "captcha_image" && req.image_selector) {
          const image = p.locator(req.image_selector);
          if (await image.count() !== 1 || !await image.isVisible()) return fail(id, "captcha_image_not_ready");
          imageBase64 = (await image.screenshot({timeout:1000})).toString("base64");
        }
        return ok(id, { prepared: true, digits, url: p.url(), image_base64:imageBase64 });
      }
      case "submit_challenge":
      case "submit_otp": {
        const binding = otpTargets.get(req.token);
        if (!binding || Date.now() - binding.created > 300000) return fail(id, "otp_target_expired");
        const { p, url, inputs, digits, submit, kind, autoSubmit } = binding;
        const code = String(req.code || "").trim();
        if ((kind === "captcha_image" ? !code || code.length > 256 : !/^[a-zA-Z0-9]{1,64}$/.test(code)) || (digits && (!/^[0-9]+$/.test(code) || code.length !== digits)) || (inputs.length > 1 && code.length !== inputs.length)) return fail(id, "otp_invalid_code");
        if (p.url() !== url) return fail(id, "otp_page_changed");
        for (const input of inputs) {
          if (!await input.evaluate(el => el.isConnected) || !await input.isVisible() || !await input.isEditable()) return fail(id, "otp_target_changed");
        }
        if (submit && !await submit.evaluate(el => el.isConnected)) return fail(id, "otp_submit_changed");
        // Consume before the first write: never replay an uncertain submission.
        otpTargets.delete(req.token);
        for (let i = 0; i < inputs.length; i++) {
          await inputs[i].fill(inputs.length === 1 ? code : code[i], { timeout: 600 });
        }
        if (submit) {
          const actual = (await Promise.all(inputs.map(input => input.inputValue()))).join("");
          if (actual !== code || p.url() !== url) return fail(id, "otp_fill_not_confirmed");
          await submit.click({ timeout: 1200, noWaitAfter: true });
        }
        const result = { filled: true, submitted: !!submit || autoSubmit, status: submit || autoSubmit ? "submitted" : "filled", confirmation: "unconfirmed" };
        // A bounded observation follows the write; never replay if it fails.
        const observed = await actionResult({ ...req, observe_after: true }, p, result);
        return JSON.parse(JSON.stringify(observed).split(code).join("[redacted]"));
      }
      case "ping":
        return ok(id, {
          pong: true,
          protocol: 1,
          ops: [
            "ping", "fetch", "prepare_challenge", "submit_challenge", "prepare_otp", "submit_otp",
            "navigate",
            "snapshot",
            "click",
            "type",
            "eval",
            "close",
            "status",
            "handoff_prepare", "read_page", "tabs", "press", "select", "scroll", "wait", "upload", "download",
          ],
        });

      case "status": {
        return ok(id, {
          browser_open: !!browser,
          headed: browserHeaded,
          last_url: lastUrl,
          page_url: page ? page.url() : null,
          env_headed: envHeadedDefault(),
        });
      }

      case "handoff_prepare":
        for (const tab of tabs.values()) { for (const frame of tab.frames()) invalidateSnapshot(frame); } {
        // Bring a visible Chromium window forward for human login/OTP/captcha.
        const info = await ensureHeadedForHandoff();
        return ok(id, {
          ...info,
          message:
            "Browser is headed (visible). Complete login/OTP/captcha in the window, then resume in the terminal.",
        });
      }

      case "navigate": {
        const url = req.url;
        if (!url) return fail(id, "navigate: missing url");
        const p = await target(req);
        const resp = await p.goto(String(url), {
          waitUntil: "domcontentloaded",
          timeout: req.timeout_ms ?? 30000,
        });
        lastUrl = p.url();
        return ok(id, {
          url: lastUrl,
          title: await p.title(),
          status: resp ? resp.status() : null,
        });
      }

      case "snapshot":
      case "dom": {
        const p = await target(req);
        const snap = await buildSnapshot(p);
        lastUrl = snap.url;
        return ok(id, {
          url: snap.url,
          title: snap.title,
          count: snap.count,
          snapshot_id: snap.snapshot_id,
          snapshot: snap.text.slice(0, 24000),
          truncated: snap.text.length > 24000,
          nodes: snap.nodes.slice(0, 200),
          frames: snap.frames,
        });
      }

      case "click": {
        if (
          !req.ref &&
          !req.selector &&
          !req.role &&
          !req.text &&
          !req.label &&
          !req.placeholder
        ) {
          return fail(
            id,
            "click requires selector, role(+name), text, label, or placeholder"
          );
        }
        const p = await target(req);
        const loc = await resolveLocator(p, req);
        const legacyPopup = process.env.LAZYBOY_FAST_POPUP === "0";
        const popupEvent = req.expect_popup === true || legacyPopup
          ? page.waitForEvent("popup", {timeout: legacyPopup ? 750 : (req.timeout_ms ?? 10000)}).catch(()=>null)
          : null;
        await loc.click({ timeout: req.timeout_ms ?? 10000 });
        const popup=await popupEvent;
        if(popup) { registerTab(popup); await popup.waitForLoadState("domcontentloaded",{timeout:10000}).catch(()=>{}); }
        lastUrl = p.url();
        return actionResult(req, popup || p, { clicked: true, url: lastUrl,
          popup_tab_id:popup?[...tabs].find(([,p])=>p===popup)?.[0]:null,
          ...(req.expect_popup === true && !popup ? {popup_error: "expected popup did not arrive; click already executed, inspect tabs before retrying"} : {}) });
      }

      case "type": {
        const text = req.text ?? req.value;
        if (text == null) return fail(id, "type: missing text");
        const p = await target(req);
        if (req.expected_origin && new URL(p.url()).origin !== req.expected_origin) {
          return fail(id, "secret destination origin changed; inspect the login page again");
        }
        const loc = await resolveLocator(p, req);
        if (req.clear !== false) {
          await loc.fill(String(text), { timeout: req.timeout_ms ?? 10000 });
        } else {
          await loc.type(String(text), { timeout: req.timeout_ms ?? 10000 });
        }
        lastUrl = p.url();
        return actionResult(req, p, { typed: true, url: lastUrl });
      }

      case "read_page": {
        const p=await target(req);
        const offset=Math.max(0,Number(req.offset)||0), limit=Math.min(32000,Math.max(1,Number(req.limit)||12000));
        const text=await p.locator("body").innerText();
        const links=await p.locator("a[href]").evaluateAll(els=>els.slice(0,200).map(e=>({text:(e.innerText||"").slice(0,200),url:e.href})));
        const chars=Array.from(text);
        return ok(id,{url:p.url(),title:await p.evaluate(()=>document.title),text:chars.slice(offset,offset+limit).join(""),offset,next_offset:Math.min(chars.length,offset+limit),truncated:chars.length>offset+limit,links});
      }
      case "tabs": {
        await ensurePage();
        const action=req.action||"list";
        if(action==="new") { page=await context.newPage(); registerTab(page); }
        else if(action==="switch") { await target(req); await page.bringToFront(); }
        else if(action==="close") { await target(req); await page.close(); }
        else if(action!=="list") throw new Error("invalid tabs action");
        const items=[];
        for(const [tab_id,p] of tabs) { items.push({tab_id,url:p.url(),title:await p.title(),active:p===page,parent_tab_id:[...tabs].find(([,parent])=>parent===popupParents.get(p))?.[0] ?? null}); }
        return ok(id,{tabs:items});
      }
      case "press": {
        if(!req.key) throw new Error("press requires key");
        const p=await target(req);
        if(req.ref||req.selector||req.role||req.label||req.placeholder) await (await resolveLocator(p,req)).press(String(req.key));
        else await page.keyboard.press(String(req.key));
        return actionResult(req,p,{pressed:req.key,url:p.url()});
      }
      case "select": {
        const p=await target(req);
        if(!Array.isArray(req.values)) throw new Error("select requires values array");
        const selected=await (await resolveLocator(p,req)).selectOption(req.values.map(String));
        return actionResult(req,p,{selected,url:p.url()});
      }
      case "scroll": {
        const p=await target(req); const delta=Number(req.delta_y)||600;
        if(req.selector) await (await resolveLocator(p,req)).evaluate((el,y)=>el.scrollBy(0,y),delta);
        else await p.evaluate(y=>window.scrollBy(0,y),delta);
        return actionResult(req,p,{scrolled:delta,url:p.url()});
      }
      case "wait": {
        const p=await target(req); const timeout=Math.min(30000,Math.max(1,Number(req.timeout_ms)||10000));
        if(req.url) await p.waitForURL(String(req.url),{timeout});
        else await (await resolveLocator(p,req)).waitFor({state:req.state||"visible",timeout});
        return ok(id,{ready:true,url:p.url()});
      }
      case "upload": {
        const p=await target(req); const path=await workspacePath(req.path,true);
        await (await resolveLocator(p,req)).setInputFiles(path);
        return ok(id,{uploaded:true,path,url:p.url()});
      }
      case "download": {
        const p=await target(req); const path=await workspacePath(req.path,false);
        // Attach rejection handling immediately if clicking fails before the event arrives.
        const downloadPromise=page.waitForEvent("download",{timeout:30000});
        downloadPromise.catch(()=>{});
        await (await resolveLocator(p,req)).click();
        const download=await downloadPromise;
        await download.saveAs(path);
        const failure=await download.failure(); if(failure) throw new Error(failure);
        return ok(id,{downloaded:true,path,bytes:(await stat(path)).size,suggested_filename:download.suggestedFilename(),url:p.url()});
      }

      case "eval": {
        const p = await target(req);
        const expression = req.expression ?? req.js ?? req.code;
        if (!expression) return fail(id, "eval: missing expression");
        // Evaluate as expression body; fail closed on throw.
        const result = await p.evaluate((expr) => {
          // eslint-disable-next-line no-eval
          return eval(expr);
        }, String(expression));
        let serialized;
        try {
          serialized = JSON.parse(JSON.stringify(result));
        } catch {
          serialized = String(result);
        }
        return ok(id, { result: serialized, url: p.url() });
      }

      case "close": {
        if (context) {
          await persistContext();
    if (!remoteCdp) await context.close().catch(() => {});
    if (browser) await browser.close().catch(() => {});
        }
        browser = null;
        context = null;
        page = null;
        browserHeaded = false;
        return ok(id, { closed: true, last_url: lastUrl });
      }

      default:
        return fail(id, `unknown op: ${op}`);
    }
  } catch (e) {
    if (["submit_otp", "prepare_otp", "submit_challenge", "prepare_challenge"].includes(op)) return fail(id, "otp_action_failed: inspect the original page before requesting a fresh code");
    const extra = {};
    if (e && e.code) extra.code = e.code;
    return fail(id, e.message || e, extra);
  } finally {
    for (const handle of actionHandles) await handle.dispose().catch(() => {});
    actionHandles.clear();
  }
}

async function persistContext() {
  if(context && process.env.LAZYBOY_BROWSER_PROFILE) {
    const path=resolve(process.env.LAZYBOY_BROWSER_PROFILE,"lazyboy-state.json");
    await context.storageState({path});
    await chmod(path,0o600);
  }
}

async function workspacePath(value,existing) {
  if(!value) throw new Error("missing workspace file path");
  const root=await realpath(process.cwd());
  const path=resolve(root,String(value));
  const rel=relative(root,path);
  if(rel.startsWith("..")||isAbsolute(rel)) throw new Error("path outside workspace");
  let ancestor=existing?path:dirname(path);
  while(true) { try { const real=await realpath(ancestor); const r=relative(root,real); if(r.startsWith("..")||isAbsolute(r)) throw new Error("symlink outside workspace"); break; } catch(e) { if(e.code!=="ENOENT") throw e; const parent=dirname(ancestor); if(parent===ancestor) throw e; ancestor=parent; } }
  if(!existing) { try { await lstat(path); throw new Error("download target exists"); } catch(e) { if(e.code!=="ENOENT") throw e; } await mkdir(dirname(path),{recursive:true}); }
  return path;
}

function parseArgs(argv) {
  const out = { cmd: null, selfTest: false, daemon: true };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--self-test") out.selfTest = true;
    else if (a === "--cmd" || a === "-c") {
      out.cmd = argv[++i];
      out.daemon = false;
    } else if (a === "--daemon") out.daemon = true;
    else if (a === "--help" || a === "-h") out.help = true;
  }
  return out;
}

async function runSelfTest() {
  const cases = [];

  // Protocol: ping without playwright
  let r = await handle({ id: "t1", op: "ping" });
  cases.push(["ping", r.ok === true && r.pong === true]);
  cases.push([
    "ping_lists_handoff_prepare",
    Array.isArray(r.ops) && r.ops.includes("handoff_prepare"),
  ]);

  // Missing fields fail closed
  r = await handle({ id: "t2", op: "navigate" });
  cases.push(["navigate_missing_url", r.ok === false && r.blocked === true]);

  r = await handle({ id: "t3", op: "type", selector: "#x" });
  // ensurePage may fail if no playwright — either blocked missing text first
  // type checks text before ensurePage... actually type checks text then ensurePage then resolveLocator
  // missing text → fail before playwright
  cases.push(["type_missing_text", r.ok === false]);

  r = await handle({ id: "t4", op: "nope" });
  cases.push(["unknown_op", r.ok === false]);

  await handle({ id: "t4b", op: "close" });
  r = await handle({ id: "t5", op: "status" });
  cases.push(["status", r.ok === true && r.browser_open === false]);

  const failed = cases.filter(([, okv]) => !okv);
  const summary = {
    ok: failed.length === 0,
    cases: Object.fromEntries(cases),
    failed: failed.map(([n]) => n),
  };
  console.log(JSON.stringify(summary));
  process.exit(failed.length === 0 ? 0 : 1);
}

async function main() {
  const args = parseArgs(process.argv);
  if (args.help) {
    console.log(
      JSON.stringify({
        ok: true,
        usage:
          "browser_helper.mjs [--self-test] | [--cmd JSON] | (JSONL on stdin)",
        install: INSTALL_HINT,
      })
    );
    return;
  }
  if (args.selfTest) {
    await runSelfTest();
    return;
  }
  if (args.cmd) {
    let req;
    try {
      req = JSON.parse(args.cmd);
    } catch (e) {
      console.log(JSON.stringify(fail(null, `invalid --cmd JSON: ${e.message}`)));
      process.exit(1);
      return;
    }
    const res = await handle(req);
    await persistContext();
    await handle({id:"shutdown",op:"close"});
    console.log(JSON.stringify(res));
    process.exit(res.ok ? 0 : 1);
    return;
  }

  // JSONL daemon
  const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of rl) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    let req;
    try {
      req = JSON.parse(trimmed);
    } catch (e) {
      console.log(
        JSON.stringify(fail(null, `invalid JSON line: ${e.message}`))
      );
      continue;
    }
    const res = await handle(req);
    if (req.op !== "poll_challenge") await persistContext();
    console.log(JSON.stringify(res));
    if (req.op === "close" || req.command === "close") {
      // keep process alive unless parent closes stdin
    }
  }
  if (context) {
    await persistContext();
    if (!remoteCdp) await context.close().catch(() => {});
    if (browser) await browser.close().catch(() => {});
  }
}

let shuttingDown = false;
async function shutdownSignal() {
  if (shuttingDown) return;
  shuttingDown = true;
  const deadline = setTimeout(() => process.exit(1), 5000);
  try { await handle({id:"shutdown",op:"close"}); }
  finally { clearTimeout(deadline); process.exit(0); }
}
process.on("SIGTERM", shutdownSignal);
process.on("SIGINT", shutdownSignal);

main().catch(async (e) => {
  await handle({id:"shutdown",op:"close"}).catch(() => {});
  console.error(JSON.stringify(fail(null, e.message || e)));
  process.exit(1);
});
