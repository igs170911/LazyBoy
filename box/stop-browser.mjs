// Browser.close runs Chromium's normal shutdown path and flushes profile data.
import { chromium } from '/opt/lazyboy/playwright/node_modules/playwright/index.mjs';
import { readdir, readFile, writeFile, rename } from 'node:fs/promises';
import { setTimeout as delay } from 'node:timers/promises';
const deadline = setTimeout(() => process.exit(1), 8000);
try {
  const browser = await chromium.connectOverCDP('http://127.0.0.1:9222', { timeout: 2000 });
  // Recently changed cookies can still be buffered in Chromium's network
  // service when the container exits. Checkpoint the authoritative live state.
  const path = '/home/box/chrome-profile/lazyboy-restart-cookies.json';
  const temporary = `${path}.tmp`;
  await writeFile(temporary, JSON.stringify(await browser.contexts()[0].cookies()), { mode: 0o600 });
  await rename(temporary, path);
  const disconnected = new Promise(resolve => browser.once('disconnected', resolve));
  const session = await browser.newBrowserCDPSession();
  await session.send('Browser.close').catch(() => {});
  await disconnected;
  // DevTools disconnects before the browser's storage shutdown has finished.
  for (;;) {
    let running = false;
    for (const pid of await readdir('/proc')) {
      if (!/^\d+$/.test(pid)) continue;
      const cmd = await readFile(`/proc/${pid}/cmdline`, 'utf8').catch(() => '');
      const args = cmd.split('\0');
      // The network service owns the cookie database and can outlive the main
      // browser briefly. Do not let init tear it down before its final flush.
      if (args[0]?.endsWith('/chromium')) running = true;
    }
    if (!running) break;
    await delay(50);
  }
} catch { process.exitCode = 1; }
clearTimeout(deadline);
