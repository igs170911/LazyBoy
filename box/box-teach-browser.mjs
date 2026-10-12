// Records what the human does in the shared Chromium while teaching the bot.
// Steps are semantic (role, accessible name, label, value), not coordinates, so
// the bot can turn them into a skill that survives layout changes.
// Usage: box-teach-browser.mjs <lesson dir>. Writes browser.jsonl + shots/b-*.jpg;
// `.paused` drops events, `.stop` ends the recording and writes `.done-browser`.
import { chromium } from '/opt/lazyboy/playwright/node_modules/playwright/index.mjs';
import { appendFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';

const dir = process.argv[2];
if (!dir) { console.error('usage: box-teach-browser.mjs <lesson dir>'); process.exit(2); }
mkdirSync(`${dir}/shots`, { recursive: true });
let count = 0;
const paused = () => existsSync(`${dir}/.paused`);

// Runs inside every frame. Capture phase so pages that stop propagation are still seen.
const SCRIPT = `(() => {
  if (window.__lbTeachInstalled) { window.__lbTeachOn = true; return; }
  window.__lbTeachInstalled = true; window.__lbTeachOn = true;
  const text = s => (s || '').replace(/\\s+/g, ' ').trim().slice(0, 80);
  const SECRET = /pass|pwd|otp|cvv|cvc|card.?num|secret|token/i;
  const secret = el => el.type === 'password' || /cc-|one-time-code|current-password|new-password/.test(el.autocomplete || '') || SECRET.test((el.name || '') + ' ' + (el.id || ''));
  const INTERACTIVE = 'a[href],button,input,select,textarea,summary,label,[role=button],[role=link],[role=menuitem],[role=tab],[role=option],[role=checkbox],[role=radio],[role=switch],[role=combobox],[contenteditable=""],[contenteditable=true],[onclick]';
  const ROLE = el => {
    const r = el.getAttribute('role'); if (r) return r;
    const t = el.tagName.toLowerCase(), type = (el.getAttribute('type') || '').toLowerCase();
    if (t === 'a') return 'link'; if (t === 'button' || t === 'summary') return 'button';
    if (t === 'select') return 'combobox'; if (t === 'textarea') return 'textbox';
    if (t === 'input') return ({checkbox:'checkbox',radio:'radio',submit:'button',button:'button',reset:'button',image:'button',search:'searchbox',range:'slider',file:'file'})[type] || 'textbox';
    if (t === 'img') return 'img'; if (/^h[1-6]$/.test(t)) return 'heading';
    if (el.isContentEditable) return 'textbox';
    return t;
  };
  const labelOf = el => {
    if (el.labels && el.labels[0]) return text(el.labels[0].innerText);
    const by = el.getAttribute('aria-labelledby');
    if (by) return text(by.split(/\\s+/).map(id => document.getElementById(id)?.innerText || '').join(' '));
    return '';
  };
  const nameOf = el => text(el.getAttribute('aria-label')) || labelOf(el) || text(el.getAttribute('placeholder')) || text(el.getAttribute('title')) || text(el.getAttribute('alt'))
    || (['INPUT','SELECT','TEXTAREA'].includes(el.tagName) ? '' : text(el.innerText)) || (el.type === 'submit' || el.type === 'button' ? text(el.value) : '');
  const css = el => {
    if (el.id && !/\\d{3,}|[:.]/.test(el.id)) return '#' + CSS.escape(el.id);
    for (const a of ['data-testid','data-test','name','aria-label','placeholder']) {
      const v = el.getAttribute(a); if (v && v.length < 60) return el.tagName.toLowerCase() + '[' + a + '="' + v.replace(/"/g, '\\\\"') + '"]';
    }
    const parts = []; let n = el;
    while (n && n.nodeType === 1 && parts.length < 4) {
      let p = n.tagName.toLowerCase(); const sib = n.parentElement ? [...n.parentElement.children].filter(c => c.tagName === n.tagName) : [];
      if (sib.length > 1) p += ':nth-of-type(' + (sib.indexOf(n) + 1) + ')';
      parts.unshift(p); n = n.parentElement;
    }
    return parts.join(' > ');
  };
  const describe = el => {
    const r = el.getBoundingClientRect();
    return { role: ROLE(el), name: nameOf(el), label: labelOf(el), tag: el.tagName.toLowerCase(), type: el.getAttribute('type') || '',
      selector: css(el), href: el.tagName === 'A' ? el.href : '', box: window === window.top ? [r.x, r.y, r.width, r.height].map(Math.round) : null,
      viewport: [innerWidth, innerHeight], frame: window === window.top ? '' : location.href };
  };
  const send = ev => { if (window.__lbTeachOn && window.__lazyboyTeach) window.__lazyboyTeach({ ...ev, page_url: location.href, page_title: document.title }).catch(() => {}); };
  const valueOf = el => secret(el) ? '••••' : el.isContentEditable ? text(el.innerText).slice(0, 500) : String(el.value ?? '').slice(0, 500);
  const pending = new Map();
  const flush = el => { const t = pending.get(el); if (!t) return; clearTimeout(t); pending.delete(el); send({ kind: 'input', target: describe(el), value: valueOf(el), secret: secret(el), hidden: secret(el) ? String(el.value || '') : undefined }); };
  addEventListener('pointerdown', e => {
    if (e.button !== 0) return;
    const raw = e.composedPath ? e.composedPath()[0] : e.target; if (!(raw instanceof Element)) return;
    const el = raw.closest(INTERACTIVE) || raw;
    for (const other of [...pending.keys()]) flush(other);
    if (el.tagName === 'SELECT' || (el.tagName === 'INPUT' && !['checkbox','radio','submit','button','reset','image','file'].includes(el.type)) || el.tagName === 'TEXTAREA') return;
    send({ kind: 'click', target: describe(el), checked: 'checked' in el && ['checkbox','radio'].includes(el.type) ? !el.checked : undefined });
  }, true);
  addEventListener('input', e => {
    const el = e.target; if (!(el instanceof Element) || el.tagName === 'SELECT' || ['checkbox','radio'].includes(el.type)) return;
    clearTimeout(pending.get(el)); pending.set(el, setTimeout(() => flush(el), 1200));
  }, true);
  addEventListener('change', e => {
    const el = e.target; if (!(el instanceof Element)) return;
    if (el.tagName === 'SELECT') send({ kind: 'select', target: describe(el), value: [...el.selectedOptions].map(o => text(o.text)).join(', ') });
    else if (el.type === 'file') send({ kind: 'upload', target: describe(el), value: [...(el.files || [])].map(f => f.name).join(', ') });
    else if (pending.has(el)) flush(el);
  }, true);
  addEventListener('keydown', e => {
    const el = e.target;
    if (el instanceof Element && pending.has(el) && (e.key === 'Enter' || e.key === 'Tab')) flush(el);
    const combo = (e.ctrlKey || e.metaKey) && !['Control','Meta','Shift','Alt'].includes(e.key) && !['c','v','x','a','z'].includes(e.key.toLowerCase());
    // Keys aimed at the page itself (Ctrl+L, Ctrl+T...) drive Chromium's own UI, which the next navigation records.
    if (!(el instanceof Element) || el === document.body || el === document.documentElement) return;
    if (e.key === 'Enter' || e.key === 'Escape' || combo) send({ kind: 'key', key: (e.ctrlKey ? 'Ctrl+' : '') + (e.metaKey ? 'Meta+' : '') + (e.shiftKey && combo ? 'Shift+' : '') + e.key, target: el instanceof Element ? describe(el) : null });
  }, true);
  // Scrolling moves what the next click's screenshot should show.
  let settle; addEventListener('scroll', () => { clearTimeout(settle); settle = setTimeout(() => send({ kind: 'settle' }), 600); }, true);
  addEventListener('submit', e => { for (const other of [...pending.keys()]) flush(other); send({ kind: 'submit', target: describe(e.target) }); }, true);
})();`;

const write = row => appendFileSync(`${dir}/browser.jsonl`, JSON.stringify(row) + '\n');

async function shot(page) {
  const name = `shots/b-${String(++count).padStart(4, '0')}.jpg`;
  try { await page.screenshot({ path: `${dir}/${name}`, type: 'jpeg', quality: 60, timeout: 3000, animations: 'allow', caret: 'initial' }); return name; }
  catch { return null; }
}

// Values typed into password-like fields never reach disk, and are scrubbed from
// URLs and titles too (a GET form puts them in the query string).
const secrets = new Set();
const scrub = value => {
  if (typeof value !== 'string') return value;
  for (const s of secrets) for (const form of [s, encodeURIComponent(s), encodeURIComponent(s).replace(/%20/g, '+')]) value = value.split(form).join('••••');
  return value;
};

// The latest screenshot of each page is what the user saw before their next
// action, so clicks get the frame from before the page reacted to them.
const latest = new WeakMap(), refreshing = new WeakMap();
function refresh(page, wait = 700) {
  clearTimeout(refreshing.get(page));
  refreshing.set(page, setTimeout(async () => { if (!stopping && !paused()) { const name = await shot(page); if (name) latest.set(page, name); } }, wait));
}

async function record(page, row) {
  if (paused() || stopping) return;
  const { hidden, page_url, page_title, ...event } = row;
  if (hidden && hidden.length >= 3) secrets.add(hidden);
  if (event.kind === 'settle') { refresh(page, 0); return; }
  const entry = { at: Date.now(), source: 'browser', url: page_url || page.url(), title: page_title, ...event };
  if (['navigate', 'tab', 'input'].includes(event.kind)) entry.shot = await shot(page);
  else entry.shot = latest.get(page) || await shot(page);
  if (!entry.title) try { entry.title = await page.title(); } catch {}
  entry.url = scrub(entry.url); entry.title = scrub(entry.title);
  if (entry.shot) latest.set(page, entry.shot);
  write(entry);
  refresh(page);
}

const pages = new WeakSet();
function attach(page) {
  if (pages.has(page)) return; pages.add(page);
  for (const frame of page.frames()) frame.evaluate(SCRIPT).catch(() => {});
  refresh(page, 0);
  page.on('framenavigated', async frame => {
    if (frame !== page.mainFrame() || !/^https?:|^file:/.test(frame.url())) return;
    await page.waitForLoadState('domcontentloaded', { timeout: 5000 }).catch(() => {});
    await delay(400);
    await record(page, { kind: 'navigate' });
  });
}

let stopping = false;
const browser = await chromium.connectOverCDP('http://127.0.0.1:9222', { timeout: 10000 });
const context = browser.contexts()[0];
await context.exposeBinding('__lazyboyTeach', ({ page }, ev) => record(page, ev));
await context.addInitScript(SCRIPT);
for (const page of context.pages()) attach(page);
context.on('page', async page => { attach(page); await page.waitForLoadState('domcontentloaded', { timeout: 5000 }).catch(() => {}); await record(page, { kind: 'tab' }); });
writeFileSync(`${dir}/.ready-browser`, String(process.pid));

while (!existsSync(`${dir}/.stop`)) await delay(300);
stopping = true;
// Leave pages as they were: the injected listeners go quiet once this session ends.
for (const page of context.pages()) for (const frame of page.frames()) await frame.evaluate('window.__lbTeachOn = false').catch(() => {});
// Exit without browser.close(): dropping the CDP connection leaves the user's Chromium running.
writeFileSync(`${dir}/.done-browser`, '');
process.exit(0);
