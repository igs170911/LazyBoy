"""Sign-ins shared between bots.

Grok Bot runs every bot on one computer, so a site signed in once works for all of them. LazyBoy gives each bot a
computer of its own; this copies the browser sign-ins (cookies) of chosen sites from one bot's browser to another's,
when the person asks. Only site names and counts ever reach the web page; the cookies go container to container.
"""
import json

from . import computers as computers_module

PLAYWRIGHT = "/opt/lazyboy/playwright/node_modules/playwright/index.mjs"
# Runs inside a bot's computer, against its live browser.
EXPORT = f"""
import {{ chromium }} from '{PLAYWRIGHT}';
setTimeout(() => process.exit(3), 20000);
const browser = await chromium.connectOverCDP('http://127.0.0.1:9222', {{ timeout: 5000 }});
const context = browser.contexts()[0];
if (!context) process.exit(2);
process.stdout.write(JSON.stringify(await context.cookies()));
process.exit(0);
"""
IMPORT = f"""
import {{ chromium }} from '{PLAYWRIGHT}';
let input = '';
for await (const chunk of process.stdin) input += chunk;
setTimeout(() => process.exit(3), 20000);
const browser = await chromium.connectOverCDP('http://127.0.0.1:9222', {{ timeout: 5000 }});
const context = browser.contexts()[0];
if (!context) process.exit(2);
const cookies = JSON.parse(input);
await context.addCookies(cookies);
process.stdout.write(String(cookies.length));
process.exit(0);
"""
SECOND_LEVEL = {"com", "co", "org", "net", "gov", "edu", "ac", "or", "ne", "go"}
LOCAL = {"localhost", "127.0.0.1"}


def site_of(domain):
    """The site a cookie belongs to: shopee.tw, momoshop.com.tw, google.com."""
    labels = str(domain or "").lstrip(".").lower().split(".")
    if len(labels) >= 3 and len(labels[-1]) == 2 and labels[-2] in SECOND_LEVEL: return ".".join(labels[-3:])
    return ".".join(labels[-2:])


def belongs(cookie, sites):
    domain = str(cookie.get("domain") or "").lstrip(".").lower()
    return any(domain == site or domain.endswith("." + site) for site in sites)


class Logins:
    def __init__(self, computers):
        self.computers = computers

    async def ready(self, id):
        status = await self.computers.status(id, refresh=True)
        if not status.get("ready"): await self.computers.start(id)

    async def cookies(self, id):
        await self.ready(id)
        code, out, err = await computers_module.docker("exec", self.computers.name(id), "node", "--input-type=module", "-e", EXPORT, check=False, timeout=40)
        if code: raise RuntimeError("讀不到這台電腦的瀏覽器登入，請確認電腦已開啟" + (f"（{err.strip()[-200:]}）" if err.strip() else ""))
        try: rows = json.loads(out)
        except ValueError: raise RuntimeError("瀏覽器回傳的登入資料無法讀取") from None
        return [row for row in rows if isinstance(row, dict) and str(row.get("domain") or "").lstrip(".") not in LOCAL]

    async def sites(self, id):
        """Signed-in sites in this bot's browser: names and cookie counts only."""
        grouped = {}
        for cookie in await self.cookies(id):
            site = site_of(cookie.get("domain"))
            if site: grouped[site] = grouped.get(site, 0) + 1
        return [{"site": site, "cookies": count} for site, count in sorted(grouped.items(), key=lambda kv: (-kv[1], kv[0]))]

    async def copy(self, source, target, sites):
        if source == target: raise ValueError("請選另一個 Bot")
        chosen = {str(site).lower() for site in sites or [] if str(site).strip()}
        if not chosen: raise ValueError("請選要分享的網站")
        cookies = [c for c in await self.cookies(source) if belongs(c, chosen)]
        if not cookies: raise ValueError("這些網站在來源 Bot 的瀏覽器裡沒有登入資料")
        await self.ready(target)
        code, out, err = await computers_module.docker("exec", "-i", self.computers.name(target), "node", "--input-type=module", "-e", IMPORT,
                                                       check=False, timeout=40, input=json.dumps(cookies).encode())
        if code: raise RuntimeError("寫入對方瀏覽器失敗" + (f"（{err.strip()[-200:]}）" if err.strip() else ""))
        return {"copied": int(out.strip() or 0), "sites": sorted(chosen)}
