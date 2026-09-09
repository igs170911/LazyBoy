#!/usr/bin/env node
// Capture README hero screenshots from a running LazyBoy instance.
//
// The banner must show the real app, so this drives a real browser against a
// real stack instead of painting a mock-up. No npm dependencies: it speaks
// Chrome DevTools Protocol over the browser WebSocket (Node 22+).
//
//   node scripts/capture-hero.mjs --out docs/hero/agent.png
//   node scripts/capture-hero.mjs --out docs/hero/room.png --pick 測試聊天
//   node scripts/capture-hero.mjs --out docs/hero/desktop.png --selector .side-card
//
// A capture that goes into the repository must not photograph whatever the
// agent's browser happens to have open. `--hide` blanks a region (its layout
// box stays, so nothing reflows) and `--anchor` prints where that region sits
// inside the shot, which is what docs/hero.html uses to overlay a clean panel.
//
//   node scripts/capture-hero.mjs --pick 測試聊天 --hide .side-card --anchor .side-card \
//     --out docs/hero/room.png
//   node scripts/capture-hero.mjs --pick 阿狗 --selector .side-card --out docs/hero/desktop.png
//
// Environment:
//   LB_URL     stack to photograph (default http://127.0.0.1:3101)
//   LB_TOKEN   login token (default: LAZYBOY_APP_TOKEN from .env)
//   CHROME     browser binary (default: discovered, see findChrome)
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const option = (name, fallback) => {
  const index = process.argv.indexOf(`--${name}`);
  return index > -1 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};

const config = {
  base: (process.env.LB_URL || 'http://127.0.0.1:3101').replace(/\/$/, ''),
  out: option('out', 'docs/hero/app.png'),
  pick: option('pick', ''),
  selector: option('selector', '.app-shell'),
  hide: option('hide', ''),
  anchor: option('anchor', ''),
  width: Number(option('width', 1480)),
  // 1480x650 keeps the 1184x520 image box in docs/hero.html free of cropping.
  height: Number(option('height', 650)),
  scale: Number(option('scale', 2)),
  settle: Number(option('settle', 6000)),
};

function findChrome() {
  const candidates = [
    process.env.CHROME,
    ...['chromium', 'chromium-browser', 'google-chrome', 'google-chrome-stable', 'brave', 'microsoft-edge']
      .map((name) => process.env.PATH.split(path.delimiter).map((dir) => path.join(dir, name)).find((bin) => fs.existsSync(bin))),
    '/Applications/Brave Browser.app/Contents/MacOS/Brave Browser',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  ].filter(Boolean);
  for (const candidate of candidates) if (fs.existsSync(candidate)) return candidate;
  const playwright = path.join(os.homedir(), '.cache/ms-playwright');
  if (fs.existsSync(playwright)) {
    for (const build of fs.readdirSync(playwright).sort().reverse()) {
      for (const dir of ['chrome-linux64', 'chrome-mac']) {
        const bin = path.join(playwright, build, dir, 'chrome');
        if (fs.existsSync(bin)) return bin;
      }
    }
  }
  throw new Error('no Chromium found; set CHROME=/path/to/chrome');
}

function readToken() {
  if (process.env.LB_TOKEN) return process.env.LB_TOKEN;
  const file = path.join(root, '.env');
  const match = fs.existsSync(file) ? fs.readFileSync(file, 'utf8').match(/^LAZYBOY_APP_TOKEN=(.+)$/m) : null;
  if (!match) throw new Error('no login token; set LB_TOKEN or create .env with make env');
  return match[1].trim();
}

async function waitForVersion(port) {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (response.ok) return await response.json();
    } catch {
      // Chrome is still coming up.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error('Chrome did not expose a DevTools endpoint');
}

// Minimal flat CDP client: one WebSocket, promise-per-id, plus event waiting.
class Connection {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Map();
    socket.addEventListener('message', (event) => {
      const message = JSON.parse(event.data);
      if (message.id) {
        const waiter = this.pending.get(message.id);
        if (!waiter) return;
        this.pending.delete(message.id);
        if (message.error) waiter.reject(new Error(`${message.method}: ${message.error.message}`));
        else waiter.resolve(message.result);
        return;
      }
      const handlers = this.listeners.get(message.method) || [];
      this.listeners.set(message.method, handlers.filter((handler) => !handler.once));
      handlers.forEach((handler) => handler.resolve(message.params));
    });
  }

  send(method, params = {}, sessionId) {
    const id = this.nextId;
    this.nextId += 1;
    this.socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
  }

  waitFor(method, sessionId) {
    return new Promise((resolve) => {
      const handlers = this.listeners.get(method) || [];
      handlers.push({ resolve, once: true });
      this.listeners.set(method, handlers);
    });
  }
}

// Retry an evaluate call until it returns something other than null. The app
// loads its lists over the API, so a single shot races the first render.
const retry = async (attempt) => {
  for (let pass = 0; pass < 40; pass += 1) {
    const value = await attempt();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  return null;
};

const launch = async () => {
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'lazyboy-hero-'));
  const port = 9400 + Math.floor(Math.random() * 200);
  const args = [
    '--headless=new',
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${profile}`,
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-gpu',
    '--hide-scrollbars',
    '--force-color-profile=srgb',
    `--window-size=${config.width},${config.height}`,
    ...(process.getuid?.() === 0 ? ['--no-sandbox'] : []),
    'about:blank',
  ];
  const chrome = spawn(findChrome(), args, { stdio: 'ignore' });
  chrome.on('exit', (code) => {
    if (code && code !== 0) console.error(`chrome exited early with code ${code}`);
  });
  const cleanup = () => {
    chrome.kill('SIGKILL');
    fs.rmSync(profile, { recursive: true, force: true });
  };
  const version = await waitForVersion(port);
  return { cleanup, version };
};

const { cleanup, version } = await launch();
process.on('exit', cleanup);
try {
  const socket = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true });
    socket.addEventListener('error', () => reject(new Error('DevTools WebSocket failed')), { once: true });
  });
  const connection = new Connection(socket);
  const { targetId } = await connection.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await connection.send('Target.attachToTarget', { targetId, flatten: true });
  await connection.send('Page.enable', {}, sessionId);
  await connection.send(
    'Emulation.setDeviceMetricsOverride',
    { width: config.width, height: config.height, deviceScaleFactor: config.scale, mobile: false },
    sessionId,
  );

  const goto = async (url) => {
    const loaded = connection.waitFor('Page.loadEventFired', sessionId);
    await connection.send('Page.navigate', { url }, sessionId);
    await loaded;
  };

  await goto(`${config.base}/`);
  const status = await connection.send(
    'Runtime.evaluate',
    {
      expression: `fetch('/api/session',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({token:${JSON.stringify(readToken())}})}).then(r=>r.status)`,
      awaitPromise: true,
      returnByValue: true,
    },
    sessionId,
  ).then((result) => result.result?.value);
  if (status !== 200) throw new Error(`login failed with HTTP ${status}`);
  await goto(`${config.base}/`);

  if (config.pick) {
    // The sidebar fills in after the first API responses, so retry until the
    // entry is really there before clicking.
    const clicked = await retry(async () => connection.send(
      'Runtime.evaluate',
      {
        expression: `(() => {
          const label = ${JSON.stringify(config.pick)};
          const hits = [...document.querySelectorAll('button,[role="button"],li,div')].filter((el) => {
            const text = (el.innerText || '').trim();
            return text.startsWith(label) && text.length < 90 && el.offsetParent !== null
              && el.getBoundingClientRect().width < 420;
          });
          if (!hits.length) return null;
          hits.sort((a, b) => a.innerText.length - b.innerText.length)[0].click();
          return 'clicked';
        })()`,
        returnByValue: true,
      },
      sessionId,
    ).then((result) => result.result?.value));
    if (clicked !== 'clicked') throw new Error(`sidebar entry "${config.pick}" not found`);
  }

  // The noVNC panel is a live stream; give it time to paint a real desktop.
  await new Promise((resolve) => setTimeout(resolve, config.settle));

  if (config.hide) {
    const hidden = await connection.send(
      'Runtime.evaluate',
      {
        expression: `document.querySelectorAll(${JSON.stringify(config.hide)}).length`,
        returnByValue: true,
      },
      sessionId,
    ).then((result) => result.result?.value);
    // A typo in --hide would silently ship a photograph of the live desktop.
    if (!hidden) throw new Error(`--hide ${config.hide} matched nothing; refusing to capture`);
    // A stylesheet survives React re-renders; an inline style does not.
    await connection.send(
      'Runtime.evaluate',
      {
        expression: `(() => {
          const style = document.createElement('style');
          const rules = ${JSON.stringify(config.hide)}.split(',')
            .map((s) => s.trim()).filter(Boolean)
            .map((s) => s + ' { visibility: hidden !important }').join('\\n');
          style.textContent = rules;
          document.head.appendChild(style);
          return document.querySelectorAll(${JSON.stringify(config.hide)}).length;
        })()`,
        returnByValue: true,
      },
      sessionId,
    );
  }

  const rect = await connection.send(
    'Runtime.evaluate',
    {
      expression: `(() => {
        const el = document.querySelector(${JSON.stringify(config.selector)});
        if (!el) return null;
        const box = el.getBoundingClientRect();
        return { x: box.x, y: box.y, width: box.width, height: box.height };
      })()`,
      returnByValue: true,
    },
    sessionId,
  ).then((result) => result.result?.value);
  if (!rect) throw new Error(`selector ${config.selector} not found`);

  let placed = '';
  if (config.anchor) {
    const box = await connection.send(
      'Runtime.evaluate',
      {
        expression: `(() => {
          const host = document.querySelector(${JSON.stringify(config.selector)});
          const el = document.querySelector(${JSON.stringify(config.anchor)});
          if (!host || !el) return null;
          const a = el.getBoundingClientRect();
          const b = host.getBoundingClientRect();
          return { x: a.x - b.x, y: a.y - b.y, width: a.width, height: a.height };
        })()`,
        returnByValue: true,
      },
      sessionId,
    ).then((result) => result.result?.value);
    if (!box) throw new Error(`anchor ${config.anchor} not found`);
    placed = ` anchor ${box.x.toFixed(1)},${box.y.toFixed(1)} ${box.width.toFixed(1)}x${box.height.toFixed(1)}`;
  }

  const shot = await connection.send(
    'Page.captureScreenshot',
    { format: 'png', clip: { ...rect, scale: 1 }, captureBeyondViewport: true },
    sessionId,
  );
  const file = path.resolve(root, config.out);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, Buffer.from(shot.data, 'base64'));
  const bytes = fs.statSync(file).size;
  console.log(`wrote ${path.relative(root, file)} ${Math.round(rect.width * config.scale)}x${Math.round(rect.height * config.scale)} (${bytes} bytes)${placed}`);
} finally {
  cleanup();
}
