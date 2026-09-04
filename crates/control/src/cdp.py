import json, os, sys, time, socket, base64, struct, subprocess, urllib.request

def fail(msg):
    print(json.dumps({"ok": False, "error": msg}))
    sys.exit(0)

def http_json(url, timeout=2):
    try:
        with urllib.request.urlopen(url, timeout=timeout) as r:
            return json.loads(r.read().decode())
    except Exception:
        return None

class Ws:
    def __init__(self, url):
        rest = url[5:]
        hostpath = rest.split("/", 1)
        hostport = hostpath[0]
        path = "/" + (hostpath[1] if len(hostpath) > 1 else "")
        if ":" in hostport:
            host, port = hostport.rsplit(":", 1)
            port = int(port)
        else:
            host, port = hostport, 80
        self.sock = socket.create_connection((host, port), 5)
        key = base64.b64encode(os.urandom(16)).decode()
        req = (
            "GET %s HTTP/1.1\r\nHost: %s\r\nUpgrade: websocket\r\n"
            "Connection: Upgrade\r\nSec-WebSocket-Key: %s\r\n"
            "Sec-WebSocket-Version: 13\r\nOrigin: http://%s\r\n\r\n"
            % (path, hostport, key, hostport)
        )
        self.sock.sendall(req.encode())
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise RuntimeError("ws handshake closed")
            buf += chunk
        self.n = 0

    def _frame(self, data):
        n = len(data)
        hdr = bytearray([0x81])
        if n < 126:
            hdr.append(0x80 | n)
        elif n < 65536:
            hdr.append(0x80 | 126)
            hdr += struct.pack("!H", n)
        else:
            hdr.append(0x80 | 127)
            hdr += struct.pack("!Q", n)
        mask = os.urandom(4)
        hdr += mask
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
        return bytes(hdr) + masked

    def _read(self, n):
        buf = b""
        while len(buf) < n:
            chunk = self.sock.recv(n - len(buf))
            if not chunk:
                raise RuntimeError("ws eof")
            buf += chunk
        return buf

    def _read_frame(self):
        hdr = self._read(2)
        opcode = hdr[0] & 0x0F
        masked = hdr[1] & 0x80
        n = hdr[1] & 0x7F
        if n == 126:
            n = struct.unpack("!H", self._read(2))[0]
        elif n == 127:
            n = struct.unpack("!Q", self._read(8))[0]
        mask = self._read(4) if masked else b""
        payload = self._read(n)
        if masked:
            payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        return opcode, payload

    def recv_json(self):
        while True:
            opcode, payload = self._read_frame()
            if opcode == 0x8:
                raise RuntimeError("ws closed")
            if opcode == 0x9:
                continue
            if opcode in (0x1, 0x2):
                return json.loads(payload.decode())

    def call(self, method, params=None):
        self.n += 1
        msg = {"id": self.n, "method": method}
        if params:
            msg["params"] = params
        self.sock.sendall(self._frame(json.dumps(msg).encode()))
        while True:
            obj = self.recv_json()
            if obj.get("id") == self.n:
                if "error" in obj:
                    raise RuntimeError(str(obj["error"]))
                return obj.get("result") or {}

    def close(self):
        try:
            self.sock.close()
        except Exception:
            pass

def probe(port):
    return http_json("http://127.0.0.1:%s/json/version" % port) is not None

def kill_profile(profile):
    if not profile:
        return
    try:
        out = subprocess.check_output(["pgrep", "-af", "chromium"], text=True, stderr=subprocess.DEVNULL)
    except Exception:
        return
    for line in out.splitlines():
        if profile not in line or "--type=" in line:
            continue
        try:
            os.kill(int(line.split()[0]), 15)
        except Exception:
            pass

def spawn_browser(display, profile, port):
    env = os.environ.copy()
    env["DISPLAY"] = display
    if profile:
        env["LAZYBOY_BROWSER_PROFILE"] = profile
    subprocess.Popen(
        ["lazyboy-browser", "--remote-debugging-port=%s" % port, "--remote-allow-origins=*"],
        env=env,
        start_new_session=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

def connect(port):
    tabs = http_json("http://127.0.0.1:%s/json/list" % port) or []
    pages = [t for t in tabs if t.get("type") == "page" and t.get("webSocketDebuggerUrl")]
    if not pages:
        fail("no browser tab")
    pages.sort(key=lambda t: (t.get("url") or "").startswith("chrome://"), reverse=False)
    ws = Ws(pages[0]["webSocketDebuggerUrl"])
    ws.call("Runtime.enable")
    ws.call("Page.enable")
    return ws

SNAP_JS = r"""
(() => {
  const sels = 'a, button, input, textarea, select, summary, label, [role="button"], [role="link"], [role="textbox"], [role="checkbox"], [role="menuitem"], [contenteditable="true"]';
  const chromeH = Math.max(0, (window.outerHeight || 0) - (window.innerHeight || 0));
  const chromeW = Math.max(0, (window.outerWidth || 0) - (window.innerWidth || 0));
  const sx0 = (window.screenX || 0) + Math.floor(chromeW / 2);
  const sy0 = (window.screenY || 0) + chromeH;
  const seen = new Set();
  const out = [];
  let n = 1;
  for (const el of document.querySelectorAll(sels)) {
    const r = el.getBoundingClientRect();
    if (r.width < 4 || r.height < 4) continue;
    if (r.bottom < 0 || r.right < 0 || r.top > innerHeight || r.left > innerWidth) continue;
    const st = getComputedStyle(el);
    if (st.visibility === "hidden" || st.display === "none" || Number(st.opacity) === 0) continue;
    const text = (el.innerText || el.value || el.getAttribute("aria-label") || el.getAttribute("placeholder") || el.getAttribute("name") || el.tagName)
      .replace(/\s+/g, " ").trim().slice(0, 80);
    if (!text) continue;
    const key = [el.tagName, text, Math.round(r.x), Math.round(r.y)].join("|");
    if (seen.has(key)) continue;
    seen.add(key);
    el.setAttribute("data-lazyboy", String(n));
    out.push({
      id: n,
      title: text,
      tag: el.tagName.toLowerCase(),
      selector: '[data-lazyboy="' + n + '"]',
      kind: "dom",
      x: Math.max(0, Math.round(sx0 + r.x)),
      y: Math.max(0, Math.round(sy0 + r.y)),
      w: Math.round(r.width),
      h: Math.round(r.height)
    });
    n += 1;
    if (out.length >= 50) break;
  }
  const body = (document.body && document.body.innerText || "").replace(/\s+/g, " ").trim().slice(0, 3000);
  return {url: location.href, title: document.title || "", text: body, elements: out};
})()
"""

CLICK_JS = r"""
(sel) => {
  const el = document.querySelector(sel);
  if (!el) return {ok: false, error: "element gone"};
  el.scrollIntoView({block: "center", inline: "nearest"});
  const r = el.getBoundingClientRect();
  el.focus();
  el.click();
  const chromeH = Math.max(0, (window.outerHeight || 0) - (window.innerHeight || 0));
  const chromeW = Math.max(0, (window.outerWidth || 0) - (window.innerWidth || 0));
  const sx = (window.screenX || 0) + Math.floor(chromeW / 2) + r.x + r.width / 2;
  const sy = (window.screenY || 0) + chromeH + r.y + r.height / 2;
  return {ok: true, x: Math.round(sx), y: Math.round(sy)};
}
"""

def evaluate(ws, expression, args=None):
    params = {"expression": expression, "returnByValue": True, "awaitPromise": True}
    if args is not None:
        params = {
            "expression": "(%s)(%s)" % (expression, json.dumps(args)),
            "returnByValue": True,
            "awaitPromise": True,
        }
    result = ws.call("Runtime.evaluate", params)
    val = (result.get("result") or {}).get("value")
    if result.get("exceptionDetails"):
        raise RuntimeError(str(result["exceptionDetails"]))
    return val

def wait_for_visual_update(ws):
    # CDP input and DOM clicks can complete before Chromium commits the next
    # painted frame. The caller captures X11 immediately after this process
    # exits, so wait for two animation frames to keep that screenshot aligned
    # with the framebuffer streamed by VNC.
    try:
        evaluate(ws, "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))")
    except Exception:
        pass

def snapshot(ws):
    val = evaluate(ws, SNAP_JS) or {}
    return {
        "ok": True,
        "action": "snapshot",
        "url": val.get("url") or "",
        "title": val.get("title") or "",
        "text": val.get("text") or "",
        "elements": val.get("elements") or [],
    }

def pointer(display, x, y):
    try:
        subprocess.check_call(
            ["env", "DISPLAY=%s" % display, "xdotool", "mousemove", "--sync", "--", str(int(x)), str(int(y))],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except Exception:
        pass

KEYS = {
    "Return": (13, "Enter", "Enter"),
    "Enter": (13, "Enter", "Enter"),
    "Tab": (9, "Tab", "Tab"),
    "BackSpace": (8, "Backspace", "Backspace"),
    "Backspace": (8, "Backspace", "Backspace"),
    "Escape": (27, "Escape", "Escape"),
    "Esc": (27, "Escape", "Escape"),
    "Space": (32, " ", "Space"),
}

def press(ws, key):
    spec = KEYS.get(key) or KEYS.get(key.title())
    if spec:
        code, name, key_id = spec
        for typ in ("keyDown", "keyUp"):
            ws.call("Input.dispatchKeyEvent", {
                "type": typ,
                "windowsVirtualKeyCode": code,
                "key": name,
                "code": key_id,
            })
        return
    ws.call("Input.dispatchKeyEvent", {"type": "keyDown", "text": key[:1]})
    ws.call("Input.dispatchKeyEvent", {"type": "keyUp", "text": key[:1]})

def main():
    req = json.loads(sys.argv[1])
    action = req.get("action") or "snapshot"
    display = req.get("display") or ":1"
    profile = req.get("profile") or ""
    port = int(req.get("port") or 9222)
    ensure = bool(req.get("ensure"))

    if action == "probe":
        print(json.dumps({"ok": probe(port)}))
        return

    if not probe(port):
        if not ensure:
            fail("cdp unavailable")
        kill_profile(profile)
        time.sleep(0.4)
        spawn_browser(display, profile, port)
        ready = False
        for _ in range(24):
            time.sleep(0.25)
            if probe(port):
                ready = True
                break
        if not ready:
            fail("cdp unavailable")
        restarted = True
    else:
        restarted = False

    if action == "ensure":
        print(json.dumps({"ok": True, "restarted": restarted}))
        return

    ws = connect(port)
    try:
        if action == "snapshot":
            body = snapshot(ws)
            body["restarted"] = restarted
            print(json.dumps(body))
            return
        if action == "navigate":
            url = req.get("url") or ""
            if not url:
                fail("url required")
            ws.call("Page.navigate", {"url": url})
            time.sleep(1.2)
            body = snapshot(ws)
            body["restarted"] = restarted
            print(json.dumps(body))
            return
        if action == "click":
            sel = req.get("selector") or ""
            if not sel:
                fail("selector required")
            val = evaluate(ws, CLICK_JS, sel) or {}
            if not val.get("ok"):
                fail(val.get("error") or "click failed")
            pointer(display, val.get("x") or 0, val.get("y") or 0)
            wait_for_visual_update(ws)
            print(json.dumps({"ok": True, "action": "click", "selector": sel, "restarted": restarted}))
            return
        if action == "type":
            sel = req.get("selector") or ""
            text = req.get("text") or ""
            if sel:
                val = evaluate(ws, CLICK_JS, sel) or {}
                if val.get("ok"):
                    pointer(display, val.get("x") or 0, val.get("y") or 0)
            if text:
                ws.call("Input.insertText", {"text": text})
            wait_for_visual_update(ws)
            print(json.dumps({"ok": True, "action": "type", "restarted": restarted}))
            return
        if action == "press":
            key = req.get("key") or "Return"
            press(ws, key)
            wait_for_visual_update(ws)
            print(json.dumps({"ok": True, "action": "press", "key": key, "restarted": restarted}))
            return
        if action == "wait":
            ms = min(max(int(req.get("ms") or 400), 0), 5000)
            time.sleep(ms / 1000.0)
            print(json.dumps({"ok": True, "action": "wait", "ms": ms}))
            return
        fail("unsupported action")
    finally:
        ws.close()

if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        fail(str(e))
