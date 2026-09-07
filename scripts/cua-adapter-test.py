#!/usr/bin/env python3
"""Exercise the real controld adapter in a disposable desktop, not raw Cua calls."""
import argparse
import base64
import importlib.machinery
import importlib.util
import json
import os
import statistics
import time
import urllib.error
import urllib.request
from pathlib import Path

loader = importlib.machinery.SourceFileLoader("smoke", "/usr/local/bin/lazyboy-cua-smoke")
spec = importlib.util.spec_from_loader(loader.name, loader)
smoke = importlib.util.module_from_spec(spec)
loader.exec_module(smoke)
TIMINGS = {}


def api(path, body=None, display=":1", expect_error=False):
    request = urllib.request.Request(
        "http://127.0.0.1:7070" + path,
        data=None if body is None else json.dumps(body).encode(),
        headers={"Authorization": "Bearer " + os.environ["LAZYBOY_CONTROL_TOKEN"],
                 "Content-Type": "application/json", "x-lazyboy-display": display},
    )
    started = time.monotonic()
    try:
        with urllib.request.urlopen(request, timeout=150) as response:
            result = json.load(response)
    except urllib.error.HTTPError as error:
        if expect_error:
            assert error.code == 400, error.code
            return None
        raise AssertionError(f"{path}: HTTP {error.code}: {error.read()[:300]!r}") from error
    finally:
        if not expect_error:
            TIMINGS.setdefault(path + ":" + str((body or {}).get("action", "")), []).append(
                (time.monotonic() - started) * 1000
            )
    if expect_error:
        assert not result.get("ok", True), result
    else:
        assert result.get("ok", True), result
    return result


def wait_health():
    deadline = time.monotonic() + 120
    last = None
    while time.monotonic() < deadline:
        try:
            health = api("/controller/health")
            if health.get("backend") == "cua" and health.get("healthy"):
                return health
            last = health
        except Exception as error:
            last = str(error)
        time.sleep(0.5)
    raise AssertionError(f"controld/cua health was not ready: {last}")


def observe():
    result = api("/observe", {})
    png = base64.b64decode(result["png_base64"])
    assert png.startswith(b"\x89PNG"), "no desktop screenshot"
    return result


def element(result, label):
    found = [el for el in result.get("elements", []) if el["title"] == label]
    assert len(found) == 1, (label, result.get("elements"))
    return found[0]


def act(action, expect_error=False):
    return act_many([action], expect_error=expect_error)


def act_many(actions, expect_error=False):
    return api("/act", {"actions": actions, "observe": True, "settle_ms": 100},
               expect_error=expect_error)


def native_round():
    gtk = smoke.launch_gtk()
    try:
        before = observe()
        # Agent enrichment requests browser state after native observation.
        api("/browser", {"action": "snapshot", "ensure": True})
        target = element(before, "Smoke Click")
        assert target["kind"] == "a11y", target
        click = {"kind": "ref", "verb": "click", "refKind": "a11y", "target": target["selector"]}
        after = act(click)
        smoke.wait_file(smoke.CLICKED, "clicked")
        # A successful mutation invalidates the previous observation's handles.
        act(click, expect_error=True)
        after = observe()
        entry = element(after, "Smoke Entry")
        save = element(after, "Smoke Save")
        act_many([
            {"kind": "ref", "verb": "setvalue", "refKind": "a11y",
             "target": entry["selector"], "text": "中文 hello-cua"},
            {"kind": "ref", "verb": "click", "refKind": "a11y", "target": save["selector"]},
        ])
        smoke.wait_file(smoke.TYPED, "中文 hello-cua")
        act({"kind": "focus", "title": "LazyBoy Cua Smoke"})
        page = observe()
        gtk_windows = [el for el in page.get("elements", [])
                       if el.get("kind") == "window"
                       and "Cua Smoke" in el.get("title", "")
                       and "Chromium" not in el.get("title", "")]
        assert gtk_windows, page.get("elements")
        win = gtk_windows[0]
        start_x = win["x"] + 40
        start_y = win["y"] + max(int(win["h"]), 80) - 40
        last_error = None
        for _ in range(3):
            (smoke.ROOT / "cua-smoke-drag.json").unlink(missing_ok=True)
            actions = []
            for kind, offset in (("down", 0), ("move", 160), ("up", 160)):
                if actions:
                    actions.append({"kind": "wait", "ms": 40})
                actions.append({"kind": "pointer", "type": kind, "button": "left",
                                "x": start_x + offset, "y": start_y})
            api("/act", {"actions": actions, "observe": False, "settle_ms": 150})
            try:
                events = json.loads(smoke.wait_file(
                    smoke.ROOT / "cua-smoke-drag.json", "button-press-event", timeout=3))
                break
            except smoke.SmokeError as error:
                last_error = error
                act({"kind": "focus", "title": "LazyBoy Cua Smoke"})
        else:
            raise last_error
        assert "button-press-event" in events and "motion-notify-event" in events and "button-release-event" in events, events

    finally:
        gtk.terminate()
        gtk.wait(timeout=5)


def browser_round():
    page = api("/browser", {"action": "navigate", "url": smoke.FIXTURE_URL, "ensure": True})
    target = element(page, "Smoke Click")
    page = api("/browser", {"action": "click", "selector": target["selector"]})
    assert "clicked-ok" in page["text"], page
    entry = element(page, "Smoke Entry")
    page = api("/browser", {"action": "type", "selector": entry["selector"], "text": "中文 hello-cua"})
    save = element(page, "Smoke Save")
    page = api("/browser", {"action": "click", "selector": save["selector"]})
    assert "typed:中文 hello-cua" in page["text"], page
    api("/browser", {"action": "click", "selector": "p999999:999999"}, expect_error=True)
    # The browser and human view are the same existing profile/window.
    assert smoke.find_browser_window(smoke.list_windows())
    assert smoke.run(["xdpyinfo", "-display", ":1"]).returncode == 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repeat", type=int, default=10)
    parser.add_argument("--check-persistence", action="store_true")
    args = parser.parse_args()
    assert args.repeat > 0
    smoke.REPORT.mkdir(parents=True, exist_ok=True)
    smoke.wait_ready(120)
    health = wait_health()
    server = smoke.start_fixture_server()
    summary = {"backend": health, "requested": args.repeat, "passed": 0}
    try:
        if args.check_persistence:
            page = api("/browser", {"action": "navigate", "url": smoke.FIXTURE_URL, "ensure": True})
            blob = (page.get("title") or "") + "\n" + (page.get("text") or "")
            assert "session-retained" in blob, page
            summary.update(requested=1, passed=1, persistent_cookie=True)
            print("persistent browser cookie survived lifecycle change", flush=True)
            return
        for i in range(args.repeat):
            native_round()
            browser_round()
            summary["passed"] += 1
            print(f"adapter iteration {i + 1}/{args.repeat} passed", flush=True)
    finally:
        server.terminate()
        server.wait(timeout=5)
        summary["timings_ms"] = {key: {"count": len(values), "median": statistics.median(values),
                                        "max": max(values)} for key, values in TIMINGS.items()}
        (smoke.REPORT / ("persistence.json" if args.check_persistence else "adapter.json")).write_text(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
