// Consume the last clean shutdown checkpoint before the browser is released.
import { chromium } from '/opt/lazyboy/playwright/node_modules/playwright/index.mjs';
import { readFile, unlink } from 'node:fs/promises';
import { setTimeout as delay } from 'node:timers/promises';
const path = '/home/box/chrome-profile/lazyboy-restart-cookies.json';
const deadline = setTimeout(() => process.exit(1), 20000);
let browser;
try {
  const text = await readFile(path, 'utf8').catch(error => {
    if (error.code === 'ENOENT') return null;
    throw error;
  });
  if (text !== null) {
    const cookies = JSON.parse(text);
    if (!Array.isArray(cookies)) throw new Error('Invalid cookie checkpoint');
    const now = Date.now() / 1000;
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        // CDP's HTTP endpoint can answer while an existing profile's pages and
        // network service are still starting. Retry before releasing the launch.
        browser = await chromium.connectOverCDP('http://127.0.0.1:9222', { timeout: 5000 });
        const context = browser.contexts()[0];
        // Include deletions, so a previous logout cannot be undone by an old DB row.
        await context.clearCookies();
        await context.addCookies(cookies.filter(cookie => cookie.expires === -1 || cookie.expires > now));
        await unlink(path);
        break;
      } catch (error) {
        await browser?.close();
        browser = undefined;
        if (attempt === 2) throw error;
        await delay(500);
      }
    }
  }
} catch {
  console.error('Browser cookie checkpoint could not be restored.');
  process.exitCode = 1;
} finally {
  // Playwright disconnects from CDP here; it leaves the desktop browser open.
  await browser?.close();
  clearTimeout(deadline);
}
