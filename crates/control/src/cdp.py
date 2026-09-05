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
    """Use a maintained RFC6455 transport (fragmentation, ping/pong, handshake)."""
    def __init__(self, url):
        import websocket
        self.sock = websocket.create_connection(url, timeout=5, suppress_origin=True,
                                                http_no_proxy=["127.0.0.1", "localhost"])
        self.n = 0

    def recv_json(self):
        return json.loads(self.sock.recv())

    def call(self, method, params=None):
        self.n += 1
        self.sock.send(json.dumps({"id": self.n, "method": method, "params": params or {}}))
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            self.sock.settimeout(max(.1, deadline-time.monotonic()))
            obj = self.recv_json()
            if obj.get("id") == self.n:
                if "error" in obj:
                    raise RuntimeError(str(obj["error"]))
                return obj.get("result") or {}
        raise TimeoutError("CDP response deadline exceeded")

    def close(self):
        self.sock.close()

def probe(port):
    return http_json("http://127.0.0.1:%s/json/version" % port) is not None

def profile_alive(profile):
    if not profile:
        return False
    try:
        out = subprocess.check_output(["pgrep", "-af", "chromium"], text=True, stderr=subprocess.DEVNULL)
    except Exception:
        return False
    for line in out.splitlines():
        if ("--user-data-dir=%s" % profile) in line and "--type=" not in line:
            return True
    return False

def active_port(profile):
    # Chromium writes the DevTools port it actually bound here. Trust it over
    # our expected port so we attach to the window the human already sees.
    if not profile:
        return None
    try:
        with open(os.path.join(profile, "DevToolsActivePort")) as f:
            return int(f.readline().strip())
    except Exception:
        return None

def spawn_browser(display, profile, port):
    env = os.environ.copy()
    env["DISPLAY"] = display
    if profile:
        env["LAZYBOY_BROWSER_PROFILE"] = profile
    subprocess.Popen(
        ["lazyboy-browser", "--remote-debugging-port=%s" % port],
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
    # /json/list is ordered by last activity, so pages[0] is the tab the human
    # is looking at. Bring it to the front anyway so what the model reads and
    # clicks is always the tab shown on the live screen.
    pages.sort(key=lambda t: (t.get("url") or "").startswith("chrome://"), reverse=False)
    page = pages[0]
    if page.get("id"):
        http_json("http://127.0.0.1:%s/json/activate/%s" % (port, page["id"]))
    ws = Ws(page["webSocketDebuggerUrl"])
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
  document.querySelectorAll('[data-lazyboy]').forEach(el => el.removeAttribute('data-lazyboy'));
  const generation = crypto.randomUUID();
  const seen = new Set();
  const inView = [];
  const offView = [];
  for (const el of document.querySelectorAll(sels)) {
    const r = el.getBoundingClientRect();
    if (r.width < 4 || r.height < 4) continue;
    const st = getComputedStyle(el);
    if (st.visibility === "hidden" || st.display === "none" || Number(st.opacity) === 0) continue;
    const type = (el.getAttribute("type") || "").toLowerCase();
    let text;
    if (type === "password" || /password|secret|token|one-time-code/i.test([el.name, el.id, el.autocomplete].join(" "))) {
      text = "[protected input]";
    } else if (el.tagName === "INPUT" && (type === "radio" || type === "checkbox")) {
      // Quiz answers: the value is usually "on"; the label next to it is what
      // the model must read to pick the right option.
      const owner = (el.labels && el.labels[0]) || el.closest("label") || el.parentElement;
      const label = (owner && owner.innerText || el.getAttribute("aria-label") || el.value || "").replace(/\s+/g, " ").trim().slice(0, 70);
      text = type + " " + label + (el.checked ? " (checked)" : "");
    } else {
      text = (el.innerText || el.value || el.getAttribute("aria-label") || el.getAttribute("placeholder") || el.getAttribute("name") || el.tagName)
        .replace(/\s+/g, " ").trim().slice(0, 80);
    }
    if (!text.trim()) continue;
    const key = [el.tagName, text, Math.round(r.x), Math.round(r.y)].join("|");
    if (seen.has(key)) continue;
    seen.add(key);
    if (el.disabled || el.getAttribute("aria-disabled") === "true") text += " [disabled]";
    const visible = !(r.bottom < 0 || r.right < 0 || r.top > innerHeight || r.left > innerWidth);
    if (visible) {
      inView.push({el, text, x: Math.max(0, Math.round(sx0 + r.x)), y: Math.max(0, Math.round(sy0 + r.y)), w: Math.round(r.width), h: Math.round(r.height)});
    } else {
      // Controls outside the viewport are still clickable by id: the click
      // handler scrolls them into view. Zero size tells the desktop side not
      // to paint or pixel-click them.
      const where = r.top > innerHeight ? "below" : r.bottom < 0 ? "above" : "beside";
      // Buttons and form controls (Next, Submit, radios) matter more than the
      // hundredth body link, so they win the limited off-screen slots.
      const link = el.tagName === "A" || el.getAttribute("role") === "link";
      offView.push({el, text: text + " [" + where + " viewport]", x: 0, y: 0, w: 0, h: 0, rank: (link ? 1 : 0), dist: Math.abs(r.top > innerHeight ? r.top - innerHeight : r.bottom)});
    }
  }
  offView.sort((a, b) => a.rank - b.rank || a.dist - b.dist);
  const out = [];
  let n = 1;
  for (const item of inView.slice(0, 50).concat(offView.slice(0, 20))) {
    item.el.setAttribute("data-lazyboy", generation + "-" + n);
    out.push({
      id: n,
      title: item.text,
      tag: item.el.tagName.toLowerCase(),
      selector: '[data-lazyboy="' + generation + "-" + n + '"]',
      kind: "dom",
      x: item.x,
      y: item.y,
      w: item.w,
      h: item.h
    });
    n += 1;
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
  const style = getComputedStyle(el);
  const hit = document.elementFromPoint(r.x+r.width/2, r.y+r.height/2);
  if (el.disabled || el.getAttribute("aria-disabled") === "true" || r.width <= 0 || r.height <= 0 || style.visibility === "hidden" || style.display === "none" || !hit || !(hit === el || el.contains(hit))) {
    return {ok:false,error:"element is disabled, hidden, or covered; observe again"};
  }
  el.focus();
  el.click();
  const chromeH = Math.max(0, (window.outerHeight || 0) - (window.innerHeight || 0));
  const chromeW = Math.max(0, (window.outerWidth || 0) - (window.innerWidth || 0));
  const sx = (window.screenX || 0) + Math.floor(chromeW / 2) + r.x + r.width / 2;
  const sy = (window.screenY || 0) + chromeH + r.y + r.height / 2;
  return {ok: true, x: Math.round(sx), y: Math.round(sy)};
}
"""

STATE_JS = r"""
(sel) => {
  const el = document.querySelector(sel);
  if (!el) return {found: false};
  const disabled = !!el.disabled || el.getAttribute("aria-disabled") === "true";
  // Training sites explain the lock next to the button ("Please watch the
  // video", a countdown); surface that text so the model can decide how
  // long to wait.
  let hint = "";
  const near = el.parentElement && el.parentElement.parentElement;
  if (disabled && near) hint = (near.innerText || "").replace(/\s+/g, " ").trim().slice(0, 120);
  return {found: true, disabled, hint};
}
"""

# Pages often lock Next for a few seconds (stay timers) or until a video ends.
# A human just waits and clicks; do the same instead of making the model plan
# a wait/observe/click loop it tends to abandon.
CLICK_WAIT_MS = 45000

def wait_until_enabled(ws, sel, wait_ms=None):
    budget = CLICK_WAIT_MS if wait_ms is None else max(0, min(int(wait_ms), 120000))
    started = time.time()
    while True:
        state = evaluate(ws, STATE_JS, sel) or {}
        if not state.get("found"):
            return None
        if not state.get("disabled"):
            return time.time() - started
        if (time.time() - started) * 1000 >= budget:
            return None
        time.sleep(0.5)

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
        evaluate(ws, "new Promise(resolve => {setTimeout(resolve, 250); requestAnimationFrame(() => requestAnimationFrame(resolve));})")
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

# Injected into every page while a human demonstrates a task. It reports what
# the person did in terms of page semantics (which control, what text, which
# URL) rather than pixels, so the distilled skill can generalise. Secrets are
# masked before they leave the page.
RECORD_JS = r"""
(() => {
  if (window.__lbTeachInstalled) return;
  window.__lbTeachInstalled = true;
  const send = (ev) => { try { ev.at = Date.now(); ev.url = location.href; window.__lbTeach(JSON.stringify(ev)); } catch (e) {} };
  const clean = (s) => (s || "").replace(/\s+/g, " ").trim().slice(0, 120);
  const secretRe = /pass|pwd|secret|token|otp|cvv|card|pin\b/i;
  const isSecret = (el) => !el ? false : (el.type === "password" || secretRe.test(el.name || "") || secretRe.test(el.id || "") || secretRe.test(el.autocomplete || "") || secretRe.test(el.getAttribute && el.getAttribute("aria-label") || ""));
  const labelFor = (el) => {
    if (!el) return "";
    if (el.labels && el.labels.length) return clean(el.labels[0].innerText);
    const id = el.id && document.querySelector('label[for="' + el.id + '"]');
    if (id) return clean(id.innerText);
    return clean(el.getAttribute("aria-label") || el.placeholder || el.title || el.name || "");
  };
  const describe = (el) => {
    if (!el || el.nodeType !== 1) return null;
    const tag = el.tagName.toLowerCase();
    const d = { tag, role: el.getAttribute("role") || "", text: clean(el.innerText || el.value || el.alt || el.getAttribute("aria-label") || el.title || el.placeholder || ""), label: labelFor(el) };
    if (el.id) d.id = el.id;
    if (el.name) d.name = el.name;
    if (tag === "a" && el.href) d.href = el.href.slice(0, 200);
    if (tag === "input") d.type = el.type || "text";
    return d;
  };
  const actionable = (node) => {
    let el = node;
    for (let i = 0; el && i < 6; i++) {
      if (el.nodeType === 1) {
        const t = el.tagName.toLowerCase();
        if (["a","button","input","select","textarea","summary","label","option"].includes(t) || el.getAttribute("role") || el.onclick || el.getAttribute("tabindex") !== null || el.isContentEditable) return el;
      }
      el = el.parentNode;
    }
    return node && node.nodeType === 1 ? node : null;
  };
  send({ t: "page", title: document.title });
  document.addEventListener("click", (e) => {
    const el = actionable(e.target);
    const d = describe(el);
    if (d) send({ t: "click", el: d, x: Math.round(e.clientX), y: Math.round(e.clientY) });
  }, true);
  const pending = new Map();
  const flush = (el) => {
    pending.delete(el);
    const d = describe(el);
    if (!d) return;
    let value = el.isContentEditable ? el.innerText : (el.value || "");
    if (el.tagName === "SELECT" && el.selectedOptions && el.selectedOptions[0]) value = el.selectedOptions[0].text;
    if (el.type === "checkbox" || el.type === "radio") value = el.checked ? "checked" : "unchecked";
    send({ t: "input", el: d, value: isSecret(el) ? "[redacted]" : clean(value) });
  };
  document.addEventListener("input", (e) => {
    const el = e.target;
    if (!el || !(el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) return;
    clearTimeout(pending.get(el));
    pending.set(el, setTimeout(() => flush(el), 900));
  }, true);
  document.addEventListener("change", (e) => { const el = e.target; if (el && el.nodeType === 1) { clearTimeout(pending.get(el)); flush(el); } }, true);
  document.addEventListener("keydown", (e) => {
    const special = ["Enter","Escape","Tab"].includes(e.key) || e.ctrlKey || e.metaKey || e.altKey;
    if (!special || e.key === "Control" || e.key === "Meta" || e.key === "Alt" || e.key === "Shift") return;
    const el = document.activeElement;
    if (el && pending.has(el)) { clearTimeout(pending.get(el)); flush(el); }
    const combo = [e.ctrlKey ? "Ctrl" : "", e.metaKey ? "Meta" : "", e.altKey ? "Alt" : "", e.shiftKey ? "Shift" : "", e.key].filter(Boolean).join("+");
    send({ t: "key", key: combo, el: describe(el) });
  }, true);
  document.addEventListener("submit", (e) => { const f = e.target; send({ t: "submit", form: { action: (f && f.action || "").slice(0, 200), name: f && (f.name || f.id) || "" } }); }, true);
  let lastScroll = 0;
  window.addEventListener("scroll", () => { const now = Date.now(); if (now - lastScroll > 2000) { lastScroll = now; send({ t: "scroll", y: Math.round(window.scrollY) }); } }, true);
})()
"""

class Recorder:
    """Browser-level CDP session with flattened page sessions. Events from
    every tab are appended to a JSONL file until the process is killed."""

    def __init__(self, port, out):
        info = http_json("http://127.0.0.1:%s/json/version" % port) or {}
        url = info.get("webSocketDebuggerUrl")
        if not url:
            raise RuntimeError("browser has no DevTools endpoint")
        self.ws = Ws(url)
        self.out = open(out, "a", buffering=1)
        self.sessions = {}
        self.pending = []
        self.last = (None, 0)

    def emit(self, ev):
        ev.setdefault("at", int(time.time() * 1000))
        # Two sessions on one page (auto-attach + explicit) deliver the same
        # binding call twice; a key repeat is never that fast either.
        key = json.dumps({k: v for k, v in ev.items() if k != "at"}, sort_keys=True)
        if key == self.last[0] and ev["at"] - self.last[1] < 800:
            return
        self.last = (key, ev["at"])
        self.out.write(json.dumps(ev, ensure_ascii=False) + "\n")

    def call(self, method, params=None, session=None):
        self.ws.n += 1
        msg = {"id": self.ws.n, "method": method}
        if params:
            msg["params"] = params
        if session:
            msg["sessionId"] = session
        self.ws.sock.send(json.dumps(msg))
        while True:
            obj = self.ws.recv_json()
            if obj.get("id") == self.ws.n:
                if "error" in obj:
                    raise RuntimeError(str(obj["error"]))
                return obj.get("result") or {}
            self.pending.append(obj)

    def attach(self, session, target):
        if target.get("type") != "page" or not session:
            return
        target_id = target.get("targetId")
        if session in self.sessions:
            return
        if target_id in self.sessions.values():
            try:
                self.call("Target.detachFromTarget", {"sessionId": session})
            except Exception:
                pass
            return
        self.sessions[session] = target_id
        for method, params in (
            ("Runtime.enable", None),
            ("Page.enable", None),
            ("Runtime.addBinding", {"name": "__lbTeach"}),
            ("Page.addScriptToEvaluateOnNewDocument", {"source": RECORD_JS}),
            ("Runtime.evaluate", {"expression": RECORD_JS}),
        ):
            try:
                self.call(method, params, session)
            except Exception:
                pass

    def handle(self, obj):
        method = obj.get("method")
        params = obj.get("params") or {}
        if method == "Target.attachedToTarget":
            self.attach(params.get("sessionId"), params.get("targetInfo") or {})
        elif method == "Target.detachedFromTarget":
            self.sessions.pop(params.get("sessionId"), None)
        elif method == "Runtime.bindingCalled" and params.get("name") == "__lbTeach":
            try:
                self.emit(json.loads(params.get("payload") or "{}"))
            except Exception:
                pass
        elif method == "Page.frameNavigated":
            frame = params.get("frame") or {}
            if not frame.get("parentId"):
                self.emit({"t": "navigate", "url": frame.get("url") or ""})
        elif method == "Target.targetInfoChanged":
            info = params.get("targetInfo") or {}
            if info.get("type") == "page" and info.get("title"):
                self.emit({"t": "title", "url": info.get("url") or "", "title": info.get("title")})

    def run(self):
        self.ws.sock.settimeout(None)
        self.call("Target.setDiscoverTargets", {"discover": True})
        self.call("Target.setAutoAttach", {"autoAttach": True, "waitForDebuggerOnStart": False, "flatten": True})
        for target in (self.call("Target.getTargets") or {}).get("targetInfos", []):
            if target.get("type") == "page":
                try:
                    result = self.call("Target.attachToTarget", {"targetId": target["targetId"], "flatten": True})
                    self.attach(result.get("sessionId"), target)
                except Exception:
                    pass
        self.emit({"t": "recorder", "state": "started"})
        while True:
            while self.pending:
                self.handle(self.pending.pop(0))
            self.handle(self.ws.recv_json())

FILL_LOGIN_JS = r"""
(creds) => {
  if (!creds.expectedHost || location.protocol !== "https:" || location.hostname.toLowerCase() !== creds.expectedHost.toLowerCase()) {
    return {ok: false, error: "saved login requires the exact configured HTTPS host"};
  }
  const user = creds.username || "";
  const pass = creds.password || "";
  const inputs = Array.from(document.querySelectorAll("input"));
  const visible = (el) => {
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return s.display !== "none" && s.visibility !== "hidden" && el.type !== "hidden" && r.width > 0 && r.height > 0;
  };
  const password = inputs.find((el) => el.type === "password" && visible(el) && !el.disabled);
  if (!password) return {ok: false, error: "no password field on this page"};
  const userish = /user|email|login|account|phone|id/i;
  const username = inputs.find((el) => {
    if (el === password || !visible(el) || el.disabled) return false;
    const type = (el.type || "text").toLowerCase();
    if (["email", "tel", "url"].includes(type)) return true;
    if (type !== "text" && type !== "search") return false;
    const blob = [el.name, el.id, el.placeholder, el.autocomplete, el.getAttribute("aria-label")].join(" ");
    return userish.test(blob) || el === inputs[0];
  });
  function setValue(el, value) {
    const proto = HTMLInputElement.prototype;
    const desc = Object.getOwnPropertyDescriptor(proto, "value");
    if (desc && desc.set) desc.set.call(el, value);
    else el.value = value;
    el.dispatchEvent(new Event("input", {bubbles: true}));
    el.dispatchEvent(new Event("change", {bubbles: true}));
  }
  if (username) setValue(username, user);
  setValue(password, pass);
  return {ok: true, filledUsername: Boolean(username), submitted: false};
}
"""

def main():
    raw = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1].strip() else sys.stdin.read()
    req = json.loads(raw)
    action = req.get("action") or "snapshot"
    display = req.get("display") or ":1"
    profile = req.get("profile") or ""
    port = int(req.get("port") or 9222)
    ensure = bool(req.get("ensure"))

    if action == "probe":
        print(json.dumps({"ok": probe(port)}))
        return

    def bound_port():
        if probe(port):
            return port
        bound = active_port(profile)
        if bound and bound != port and probe(bound):
            return bound
        return None

    restarted = False
    ready_port = bound_port()
    if ready_port is None and profile_alive(profile):
        # The window may still be booting (lazyboy-screen just spawned it).
        for _ in range(12):
            time.sleep(0.25)
            ready_port = bound_port()
            if ready_port is not None:
                break
    if ready_port is not None:
        port = ready_port
    else:
        if profile_alive(profile):
            # Never kill the window the human is watching. Fall back to the
            # screenshot tools, which see exactly what the live screen shows.
            fail("browser is open but has no DevTools; use computer_observe/computer_act on it instead. Do not restart the browser.")
        if not ensure:
            fail("cdp unavailable")
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

    if action == "ensure":
        print(json.dumps({"ok": True, "restarted": restarted}))
        return

    if action == "record":
        # Long-running: the API starts this detached and kills it on stop.
        out = req.get("out") or "/tmp/lazyboy-teach.jsonl"
        Recorder(port, out).run()
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
            waited = wait_until_enabled(ws, sel, req.get("waitMs"))
            if waited is None:
                state = evaluate(ws, STATE_JS, sel) or {}
                if not state.get("found"):
                    # Ids are renumbered whenever the page changes; hand back
                    # the fresh numbering so the model does not have to ask.
                    body = snapshot(ws)
                    body.update({"ok": False, "error": "element gone: the page changed and ids were renumbered. Use the fresh element list in this result."})
                    print(json.dumps(body))
                    return
                fail("control %s is still disabled after waiting %ss (page says: %s). Use wait for longer if a video or timer must finish, then click again."
                     % (sel, int(req.get("waitMs") or CLICK_WAIT_MS) // 1000, state.get("hint") or "nothing"))
            val = evaluate(ws, CLICK_JS, sel) or {}
            if not val.get("ok"):
                fail(val.get("error") or "click failed")
            pointer(display, val.get("x") or 0, val.get("y") or 0)
            wait_for_visual_update(ws)
            out = {"ok": True, "action": "click", "selector": sel, "restarted": restarted}
            if waited >= 1.0:
                out["waitedSeconds"] = round(waited, 1)
            print(json.dumps(out))
            return
        if action == "fill_login":
            val = evaluate(ws, FILL_LOGIN_JS, {
                "expectedHost": req.get("expectedHost") or "",
                "username": req.get("username") or "",
                "password": req.get("password") or "",
            }) or {}
            if not val.get("ok"):
                fail(val.get("error") or "could not fill the login form")
            wait_for_visual_update(ws)
            print(json.dumps({
                "ok": True,
                "action": "fill_login",
                "filledUsername": bool(val.get("filledUsername")),
                "submitted": bool(val.get("submitted")),
                "restarted": restarted,
            }))
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
