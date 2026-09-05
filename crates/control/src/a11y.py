import json, os, sys, time

def fail(msg):
    print(json.dumps({"ok": False, "error": msg, "elements": []}))
    sys.exit(0)

def load_session(display):
    display = display or os.environ.get("DISPLAY") or ":1"
    if not display.startswith(":"):
        display = ":" + display
    os.environ["DISPLAY"] = display
    number = display.lstrip(":")
    dbus_file = "/tmp/lazyboy/screen-%s.dbus" % number
    runtime_file = "/tmp/lazyboy/screen-%s.runtime" % number
    try:
        addr = open(dbus_file).read().strip()
        if addr:
            os.environ["DBUS_SESSION_BUS_ADDRESS"] = addr
    except Exception:
        pass
    try:
        runtime = open(runtime_file).read().strip()
        if runtime:
            os.environ["XDG_RUNTIME_DIR"] = runtime
    except Exception:
        if number == "1":
            candidate = "/tmp/xfce-home/runtime"
        else:
            candidate = "/tmp/xfce-home-%s/runtime" % number
        if os.path.isdir(candidate):
            os.environ["XDG_RUNTIME_DIR"] = candidate

def atspi():
    import gi
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi
    try:
        Atspi.init()
    except Exception:
        pass
    try:
        Atspi.set_timeout(200, 200)
    except Exception:
        pass
    return Atspi

INTERACTIVE = {
    "push button", "toggle button", "check box", "radio button",
    "combo box", "text", "password text", "menu item", "check menu item",
    "radio menu item", "tab", "page tab", "slider", "spin button",
    "link", "tree item", "entry", "password", "button", "menu",
    "list item", "column header", "toggle",
}

BROWSER_APPS = ("chromium", "chrome", "google-chrome", "chromium-browser")

def is_browser_name(name):
    n = (name or "").lower()
    return any(token in n for token in BROWSER_APPS)

def child_count(acc):
    try:
        return int(acc.get_child_count())
    except Exception:
        return 0

def child_at(acc, i):
    try:
        return acc.get_child_at_index(i)
    except Exception:
        return None

def role_name(acc):
    try:
        return (acc.get_role_name() or "").lower()
    except Exception:
        return ""

def acc_name(acc):
    try:
        text = (acc.get_name() or "").strip()
        if text:
            return text
    except Exception:
        pass
    try:
        return (acc.get_description() or "").strip()
    except Exception:
        return ""

def state_names(Atspi, acc):
    out = []
    try:
        ss = acc.get_state_set()
    except Exception:
        return out
    for name in ("showing", "visible", "enabled", "sensitive", "checked",
                 "selected", "focused", "editable", "defunct", "expandable",
                 "expanded"):
        try:
            st = getattr(Atspi.StateType, name.upper())
            if ss.contains(st):
                out.append(name)
        except Exception:
            pass
    return out

def extents(Atspi, acc):
    try:
        ext = acc.get_extents(Atspi.CoordType.SCREEN)
        return int(ext.x), int(ext.y), int(ext.width), int(ext.height)
    except Exception:
        pass
    try:
        comp = acc.get_component_iface()
        if comp is None:
            return None
        ext = comp.get_extents(Atspi.CoordType.SCREEN)
        return int(ext.x), int(ext.y), int(ext.width), int(ext.height)
    except Exception:
        return None

def action_iface(acc):
    for getter in ("get_action_iface", "queryAction", "get_action"):
        fn = getattr(acc, getter, None)
        if not fn:
            continue
        try:
            iface = fn()
            if iface is not None:
                return iface
        except Exception:
            pass
    if hasattr(acc, "get_n_actions") and hasattr(acc, "do_action"):
        return acc
    return None

def text_ifaces(acc):
    edit = None
    text = None
    for getter in ("get_editable_text_iface", "queryEditableText"):
        fn = getattr(acc, getter, None)
        if not fn:
            continue
        try:
            edit = fn()
            if edit is not None:
                break
        except Exception:
            pass
    for getter in ("get_text_iface", "queryText"):
        fn = getattr(acc, getter, None)
        if not fn:
            continue
        try:
            text = fn()
            if text is not None:
                break
        except Exception:
            pass
    if edit is None and hasattr(acc, "insert_text"):
        edit = acc
    if text is None and hasattr(acc, "get_character_count"):
        text = acc
    return edit, text

def resolve(Atspi, path):
    desktop = Atspi.get_desktop(0)
    node = desktop
    for part in str(path).split("/"):
        if part == "":
            continue
        node = child_at(node, int(part))
        if node is None:
            return None
    return node

def grab_focus(acc):
    for getter in ("grab_focus",):
        fn = getattr(acc, getter, None)
        if fn:
            try:
                fn()
                return True
            except Exception:
                pass
    try:
        comp = acc.get_component_iface()
        if comp is not None:
            comp.grab_focus()
            return True
    except Exception:
        pass
    return False

def do_click(acc):
    action = action_iface(acc)
    if action is None:
        return grab_focus(acc) and False
    try:
        n = int(action.get_n_actions())
    except Exception:
        n = 0
    idx = 0
    prefer = ("click", "press", "activate", "jump", "open", "toggle", "select")
    for i in range(n):
        try:
            name = (action.get_action_name(i) or "").lower()
        except Exception:
            name = ""
        if name in prefer:
            idx = i
            break
    if n <= 0:
        return False
    try:
        return bool(action.do_action(idx))
    except Exception:
        return False

def set_text(acc, value):
    grab_focus(acc)
    edit, text = text_ifaces(acc)
    if edit is None:
        return False
    n = 0
    if text is not None:
        try:
            n = int(text.get_character_count())
        except Exception:
            n = 0
    try:
        edit.delete_text(0, n)
    except Exception:
        pass
    try:
        edit.insert_text(0, value, len(value))
        return True
    except Exception:
        return False

def snapshot(Atspi, include_browser):
    deadline = time.time() + 2.8
    desktop = Atspi.get_desktop(0)
    found = []
    visited = 0
    apps = child_count(desktop)
    for app_i in range(apps):
        if time.time() > deadline or len(found) >= 50:
            break
        app = child_at(desktop, app_i)
        if app is None:
            continue
        app_label = acc_name(app) or role_name(app)
        if not include_browser and is_browser_name(app_label):
            continue
        stack = [(app, str(app_i), 0)]
        while stack:
            if time.time() > deadline or len(found) >= 50 or visited > 400:
                break
            acc, path, depth = stack.pop()
            visited += 1
            states = state_names(Atspi, acc)
            if "defunct" in states:
                continue
            role = role_name(acc)
            name = acc_name(acc)
            showing = ("showing" in states) or ("visible" in states) or not states
            if role in INTERACTIVE and showing and name:
                box = extents(Atspi, acc)
                if box and box[2] >= 2 and box[3] >= 2:
                    x, y, w, h = box
                    if x + w > 0 and y + h > 0:
                        title = "%s %s" % (role, name.replace("\n", " ").strip())
                        if "checked" in states:
                            title += " (checked)"
                        if "expanded" in states:
                            title += " (expanded)"
                        if "enabled" in states and "sensitive" in states:
                            pass
                        elif states and "enabled" not in states:
                            title += " [disabled]"
                        title = title[:80]
                        found.append({
                            "id": len(found) + 1,
                            "title": title,
                            "role": role,
                            "kind": "a11y",
                            "selector": path,
                            "x": max(0, x),
                            "y": max(0, y),
                            "w": w,
                            "h": h,
                        })
            if depth >= 12:
                continue
            n = child_count(acc)
            # Walk children in reverse so index 0 is processed first with pop().
            for i in range(n - 1, -1, -1):
                child = child_at(acc, i)
                if child is None:
                    continue
                stack.append((child, "%s/%d" % (path, i), depth + 1))
    return found

def main():
    req = json.loads(sys.argv[1]) if len(sys.argv) > 1 else {}
    action = req.get("action") or "snapshot"
    display = req.get("display") or ":1"
    load_session(display)
    try:
        Atspi = atspi()
    except Exception as error:
        fail("atspi unavailable: %s" % error)
        return
    if action == "snapshot":
        include_browser = bool(req.get("includeBrowser"))
        try:
            elements = snapshot(Atspi, include_browser)
        except Exception as error:
            fail("atspi snapshot failed: %s" % error)
            return
        print(json.dumps({"ok": True, "elements": elements}))
        return
    selector = req.get("selector") or ""
    if not selector:
        fail("a11y action needs selector")
        return
    try:
        acc = resolve(Atspi, selector)
    except Exception as error:
        fail("a11y resolve failed: %s" % error)
        return
    if acc is None:
        fail("a11y element gone")
        return
    ok = False
    if action == "click":
        ok = do_click(acc)
    elif action == "type":
        ok = set_text(acc, req.get("text") or "")
    elif action == "focus":
        ok = grab_focus(acc)
    else:
        fail("unsupported a11y action")
        return
    print(json.dumps({"ok": bool(ok), "error": None if ok else "a11y %s failed" % action}))

if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        fail(str(error))
