#!/usr/bin/env python3
"""In-container Cua Driver smoke test for the LazyBoy XFCE + Xvfb desktop."""

from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

ROOT = Path("/tmp/lazyboy")
SOCKET = ROOT / "cua.sock"
REPORT = ROOT / "cua-smoke-report"
CLICKED = ROOT / "cua-smoke-clicked"
TYPED = ROOT / "cua-smoke-typed"
GTK_SCRIPT = Path("/usr/local/bin/lazyboy-cua-smoke-gtk")
HTML = Path("/usr/share/lazyboy/cua-smoke.html")
FIXTURE_PORT = 8765
FIXTURE_URL = f"http://127.0.0.1:{FIXTURE_PORT}/cua-smoke.html"
TYPED_TEXT = "hello-cua"


class SmokeError(RuntimeError):
    pass


def log(message: str) -> None:
    print(message, flush=True)


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, default=str) + "\n", encoding="utf-8")


def run(
    argv: list[str],
    timeout: int = 60,
    env: dict[str, str] | None = None,
    input_text: str | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        argv,
        check=False,
        capture_output=True,
        text=True,
        timeout=timeout,
        env=env,
        input=input_text,
    )


def parse_jsonish(text: str) -> Any:
    text = text.strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    start = text.find("{")
    end = text.rfind("}")
    if start >= 0 and end > start:
        try:
            return json.loads(text[start : end + 1])
        except json.JSONDecodeError:
            return None
    start = text.find("[")
    end = text.rfind("]")
    if start >= 0 and end > start:
        try:
            return json.loads(text[start : end + 1])
        except json.JSONDecodeError:
            return None
    return None


def walk(value: Any) -> list[Any]:
    found = [value]
    if isinstance(value, dict):
        for item in value.values():
            found.extend(walk(item))
    elif isinstance(value, list):
        for item in value:
            found.extend(walk(item))
    return found


def first_list_of_dicts(value: Any, required_key: str) -> list[dict[str, Any]]:
    for node in walk(value):
        if isinstance(node, list) and node and all(isinstance(item, dict) for item in node):
            if any(required_key in item for item in node):
                return node
        if isinstance(node, dict) and required_key in node and isinstance(node[required_key], list):
            items = node[required_key]
            if items and all(isinstance(item, dict) for item in items):
                return items
    return []


def cua_env() -> dict[str, str]:
    env = os.environ.copy()
    env.setdefault("DISPLAY", ":1")
    env.setdefault("CUA_DRIVER_RS_HOME", str(ROOT / "cua-home"))
    env["PATH"] = "/usr/local/bin:/usr/local/lib/cua-driver:" + env.get("PATH", "")
    dbus = ROOT / "screen-1.dbus"
    if dbus.exists() and "DBUS_SESSION_BUS_ADDRESS" not in env:
        env["DBUS_SESSION_BUS_ADDRESS"] = dbus.read_text(encoding="utf-8").strip()
    runtime = ROOT / "screen-1.runtime"
    if runtime.exists() and "XDG_RUNTIME_DIR" not in env:
        env["XDG_RUNTIME_DIR"] = runtime.read_text(encoding="utf-8").strip()
    env.setdefault("GTK_MODULES", "atk-bridge")
    env.setdefault("GTK_A11Y", "atspi")
    env.setdefault("NO_AT_BRIDGE", "0")
    return env


def cua_bin(args: list[str], timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return run(["cua-driver", *args], timeout=timeout, env=cua_env())


def cua_call(tool: str, payload: dict[str, Any] | None = None, extra: list[str] | None = None, timeout: int = 90) -> dict[str, Any]:
    argv = ["call", "--socket", str(SOCKET), tool]
    if extra:
        argv.extend(extra)
    argv.append(json.dumps(payload or {}))
    proc = cua_bin(argv, timeout=timeout)
    parsed = parse_jsonish(proc.stdout) or parse_jsonish(proc.stderr)
    result = {
        "tool": tool,
        "code": proc.returncode,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
        "parsed": parsed,
    }
    if proc.returncode != 0:
        raise SmokeError(f"{tool} failed ({proc.returncode}): {proc.stderr or proc.stdout}")
    combined = f"{proc.stdout or ''}\n{proc.stderr or ''}"
    if "❌" in combined:
        raise SmokeError(f"{tool} reported an error: {combined[-2000:]}")
    return result


def wait_ready(timeout: int) -> None:
    deadline = time.time() + timeout
    ready = ROOT / "ready"
    while time.time() < deadline:
        if ready.exists() and SOCKET.is_socket():
            status = cua_bin(["status", "--socket", str(SOCKET)], timeout=15)
            text = (status.stdout or "") + (status.stderr or "")
            if status.returncode == 0 and "not running" not in text.lower():
                return
        time.sleep(0.4)
    raise SmokeError("desktop or cua-driver socket was not ready")


def png_ok(path: Path) -> None:
    data = path.read_bytes()
    if len(data) < 32 or data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SmokeError(f"{path} is not a PNG ({len(data)} bytes)")


def kill_matching(pattern: str) -> None:
    run(["pkill", "-f", pattern], timeout=10)


def launch_gtk() -> subprocess.Popen[bytes]:
    kill_matching("lazyboy-cua-smoke-gtk")
    for path in (CLICKED, TYPED):
        if path.exists():
            path.unlink()
    log_path = REPORT / "gtk.log"
    log_file = log_path.open("w", encoding="utf-8")
    proc = subprocess.Popen(
        [str(GTK_SCRIPT)],
        env=cua_env(),
        stdout=log_file,
        stderr=log_file,
        start_new_session=True,
    )
    deadline = time.time() + 15
    while time.time() < deadline:
        if proc.poll() is not None:
            log_file.close()
            detail = log_path.read_text(encoding="utf-8")[-1500:]
            raise SmokeError(f"GTK smoke window exited immediately: {detail}")
        windows = list_windows()
        if find_gtk_window(windows):
            return proc
        time.sleep(0.3)
    log_file.close()
    raise SmokeError(f"GTK smoke window did not appear; windows={list_windows()}")


def start_fixture_server() -> subprocess.Popen[bytes]:
    proc = subprocess.Popen(
        [
            "python3",
            "-m",
            "http.server",
            str(FIXTURE_PORT),
            "--bind",
            "127.0.0.1",
            "--directory",
            str(HTML.parent),
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    deadline = time.time() + 8
    while time.time() < deadline:
        check = run(
            [
                "python3",
                "-c",
                f"import urllib.request; urllib.request.urlopen('{FIXTURE_URL}', timeout=1).read()",
            ],
            timeout=5,
        )
        if check.returncode == 0:
            return proc
        if proc.poll() is not None:
            raise SmokeError("fixture HTTP server exited")
        time.sleep(0.2)
    raise SmokeError("fixture HTTP server did not become ready")


def launch_browser() -> None:
    env = cua_env()
    subprocess.Popen(
        ["lazyboy-browser", FIXTURE_URL],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    deadline = time.time() + 25
    while time.time() < deadline:
        windows = list_windows()
        if find_browser_window(windows):
            return
        time.sleep(0.4)
    raise SmokeError("Chromium window did not appear")


def list_windows() -> list[dict[str, Any]]:
    result = cua_call("list_windows", {"on_screen_only": True})
    windows = first_list_of_dicts(result["parsed"], "window_id")
    if not windows:
        windows = first_list_of_dicts(result["parsed"], "title")
    return windows


def window_title(window: dict[str, Any]) -> str:
    return str(window.get("title") or window.get("app_name") or "")


def find_window(windows: list[dict[str, Any]], title_part: str) -> dict[str, Any] | None:
    needle = title_part.lower()
    for window in windows:
        if needle in window_title(window).lower():
            return window
    return None


def find_gtk_window(windows: list[dict[str, Any]]) -> dict[str, Any] | None:
    for window in windows:
        title = window_title(window)
        app = str(window.get("app_name") or "")
        if title == "LazyBoy Cua Smoke" and "chrom" not in app.lower():
            return window
    return None


def find_browser_window(windows: list[dict[str, Any]]) -> dict[str, Any] | None:
    for window in windows:
        blob = " ".join(
            str(window.get(key) or "")
            for key in ("title", "app_name", "application", "wm_class")
        ).lower()
        if "chrom" in blob or "lazyboy cua smoke" in blob:
            return window
    return None


def snapshot(pid: int, window_id: int, name: str) -> dict[str, Any]:
    out = REPORT / f"{name}.png"
    result = cua_call(
        "get_window_state",
        {
            "pid": pid,
            "window_id": window_id,
            "include_screenshot": True,
            "screenshot_out_file": str(out),
        },
        timeout=120,
    )
    parsed = result["parsed"] or {}
    elements = first_list_of_dicts(parsed, "element_index") or first_list_of_dicts(
        parsed, "element_token"
    )
    snapshot_id = None
    for node in walk(parsed):
        if isinstance(node, dict) and node.get("snapshot_id"):
            snapshot_id = node["snapshot_id"]
            break
    return {
        "result": result,
        "elements": elements,
        "snapshot_id": snapshot_id,
        "png": out if out.exists() else None,
    }


def element_by_label(elements: list[dict[str, Any]], label: str) -> dict[str, Any]:
    needle = label.lower()
    for element in elements:
        hay = " ".join(
            str(element.get(key) or "")
            for key in ("label", "name", "title", "role", "value")
        ).lower()
        if needle in hay:
            return element
    raise SmokeError(f"no accessibility element matching {label!r}")


def click_element(pid: int, window_id: int, element: dict[str, Any], snapshot_id: Any) -> None:
    payload: dict[str, Any] = {"pid": pid, "window_id": window_id}
    if element.get("element_token"):
        payload["element_token"] = element["element_token"]
    elif element.get("element_index") is not None and snapshot_id:
        payload["element_index"] = element["element_index"]
        payload["snapshot_id"] = snapshot_id
    else:
        raise SmokeError(f"element has no token/index: {element}")
    cua_call("click", payload)


def type_element(pid: int, window_id: int, element: dict[str, Any], snapshot_id: Any, text: str) -> None:
    payload: dict[str, Any] = {"pid": pid, "window_id": window_id, "text": text}
    if element.get("element_token"):
        payload["element_token"] = element["element_token"]
    elif element.get("element_index") is not None and snapshot_id:
        payload["element_index"] = element["element_index"]
        payload["snapshot_id"] = snapshot_id
    cua_call("type_text", payload)


def wait_file(path: Path, expect: str | None, timeout: float = 8.0) -> str:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if path.exists():
            text = path.read_text(encoding="utf-8").strip()
            if expect is None or expect in text:
                return text
        time.sleep(0.2)
    raise SmokeError(f"{path} did not contain {expect!r}")


def desktop_screenshot() -> None:
    out = REPORT / "desktop.png"
    cua_call(
        "get_desktop_state",
        {"screenshot_out_file": str(out)},
        extra=["--screenshot-out-file", str(out)],
    )
    if not out.exists():
        # Some builds only honour the JSON field or only the flag.
        time.sleep(0.2)
    if not out.exists():
        raise SmokeError("get_desktop_state did not write a screenshot")
    png_ok(out)


def collect_diagnostics() -> dict[str, Any]:
    version = cua_bin(["--version"])
    doctor = cua_bin(["doctor", "--json"], timeout=90)
    if doctor.returncode != 0:
        doctor = cua_bin(["doctor"], timeout=90)
    tools = cua_bin(["list-tools"], timeout=30)
    status = cua_bin(["status", "--socket", str(SOCKET)], timeout=15)
    report = {
        "version": (version.stdout or version.stderr).strip(),
        "doctor_code": doctor.returncode,
        "doctor_stdout": doctor.stdout,
        "doctor_stderr": doctor.stderr,
        "doctor_json": parse_jsonish(doctor.stdout),
        "list_tools": tools.stdout,
        "status": (status.stdout or "") + (status.stderr or ""),
        "display": os.environ.get("DISPLAY", ":1"),
        "socket": str(SOCKET),
    }
    write_json(REPORT / "diagnostics.json", report)
    (REPORT / "list-tools.txt").write_text(tools.stdout or "", encoding="utf-8")
    (REPORT / "doctor.txt").write_text(
        (doctor.stdout or "") + (doctor.stderr or ""), encoding="utf-8"
    )
    if version.returncode != 0:
        raise SmokeError("cua-driver --version failed")
    return report


def native_round(iteration: int) -> None:
    for path in (CLICKED, TYPED):
        if path.exists():
            path.unlink()
    gtk = launch_gtk()
    try:
        windows = list_windows()
        window = find_gtk_window(windows)
        if not window:
            raise SmokeError(f"GTK window missing: {windows}")
        pid = int(window["pid"])
        window_id = int(window["window_id"])
        state = snapshot(pid, window_id, f"native-{iteration}")
        click_el = element_by_label(state["elements"], "Smoke Click")
        click_element(pid, window_id, click_el, state["snapshot_id"])
        wait_file(CLICKED, "clicked")
        state = snapshot(pid, window_id, f"native-type-{iteration}")
        entry = element_by_label(state["elements"], "Smoke Entry")
        type_element(pid, window_id, entry, state["snapshot_id"], TYPED_TEXT)
        state = snapshot(pid, window_id, f"native-save-{iteration}")
        save = element_by_label(state["elements"], "Smoke Save")
        click_element(pid, window_id, save, state["snapshot_id"])
        wait_file(TYPED, TYPED_TEXT)
    finally:
        gtk.send_signal(signal.SIGTERM)
        try:
            gtk.wait(timeout=5)
        except subprocess.TimeoutExpired:
            gtk.kill()


def extract_text(value: Any) -> str:
    chunks: list[str] = []
    for node in walk(value):
        if isinstance(node, str) and 0 < len(node) < 400:
            chunks.append(node)
        elif isinstance(node, dict):
            for key in ("text", "value", "label", "name", "url", "title", "result", "ref"):
                item = node.get(key)
                if isinstance(item, str) and 0 < len(item) < 400:
                    chunks.append(item)
    return "\n".join(chunks)


def strip_heavy(value: Any) -> Any:
    if isinstance(value, dict):
        out = {}
        for key, item in value.items():
            if isinstance(item, str) and len(item) > 400:
                out[key] = f"<{len(item)} chars>"
            else:
                out[key] = strip_heavy(item)
        return out
    if isinstance(value, list):
        return [strip_heavy(item) for item in value]
    return value


def find_browser_ref(parsed: Any, needle: str) -> str | None:
    needle = needle.lower()
    for node in walk(parsed):
        if not isinstance(node, dict):
            continue
        ref = node.get("ref")
        if not isinstance(ref, str) or not ref:
            continue
        blob = " ".join(
            str(node.get(key) or "")
            for key in (
                "name",
                "label",
                "text",
                "role",
                "description",
                "accessible_name",
                "selector",
                "value",
            )
        ).lower()
        if needle in blob or needle in json.dumps(strip_heavy(node), default=str).lower():
            return ref
    return None


def browser_ids(parsed: Any) -> tuple[Any, Any]:
    target_id = None
    tab_id = None
    for node in walk(parsed):
        if not isinstance(node, dict):
            continue
        target_id = target_id or node.get("target_id")
        tab_id = tab_id or node.get("tab_id")
        tabs = node.get("tabs")
        if isinstance(tabs, list) and tabs and isinstance(tabs[0], dict):
            tab_id = tab_id or tabs[0].get("tab_id") or tabs[0].get("id")
    return target_id, tab_id


def browser_prepare(pid: int, window_id: int) -> dict[str, Any]:
    try:
        return cua_call(
            "browser_prepare",
            {
                "pid": pid,
                "window_id": window_id,
                "session": "lazyboy-smoke",
                "strategy": {"kind": "existing_profile"},
                "allow_launch": False,
            },
            timeout=180,
        )
    except SmokeError as error:
        return {"error": str(error)}


def browser_round(iteration: int) -> dict[str, Any]:
    windows = list_windows()
    window = find_browser_window(windows)
    if not window:
        launch_browser()
        windows = list_windows()
        window = find_browser_window(windows)
    if not window:
        raise SmokeError(f"no Chromium window: {windows}")
    pid = int(window["pid"])
    window_id = int(window["window_id"])
    bind_payload = {
        "pid": pid,
        "window_id": window_id,
        "include_screenshot": False,
        "session": "lazyboy-smoke",
    }
    state = cua_call("get_browser_state", bind_payload, timeout=120)
    note = extract_text(state["parsed"]) + state["stdout"]
    prepared = None
    if any(
        marker in note
        for marker in (
            "browser_requires_setup",
            "browser_consent_required",
            "requires_grant",
            "existing_profile",
        )
    ):
        prepared = browser_prepare(pid, window_id)
        write_json(REPORT / "browser-prepare.json", prepared)
        prep_text = extract_text(prepared.get("parsed")) + str(prepared.get("stdout") or "") + str(prepared.get("error") or "")
        if prepared.get("error") or "❌" in prep_text or "refused" in prep_text:
            raise SmokeError(f"browser_prepare failed: {prep_text[-2000:]}")
        state = cua_call("get_browser_state", bind_payload, timeout=120)
        note = extract_text(state["parsed"]) + state["stdout"]
    target_id, tab_id = browser_ids(state["parsed"])
    if not target_id or not tab_id:
        raise SmokeError(f"browser bind missing target/tab: {note[-2000:]}")
    cua_call(
        "browser_navigate",
        {
            "target_id": target_id,
            "tab_id": tab_id,
            "url": FIXTURE_URL,
            "session": "lazyboy-smoke",
        },
        timeout=60,
    )
    time.sleep(0.8)
    snap = cua_call(
        "get_browser_state",
        {
            "target_id": target_id,
            "tab_id": tab_id,
            "include_screenshot": False,
            "session": "lazyboy-smoke",
            "snapshot_format": "semantic_v2",
            "query": "Smoke",
        },
        timeout=90,
    )
    write_json(REPORT / f"browser-snap-{iteration}.json", strip_heavy(snap["parsed"]))
    click_ref = find_browser_ref(snap["parsed"], "smoke click")
    type_ref = find_browser_ref(snap["parsed"], "smoke entry")
    save_ref = find_browser_ref(snap["parsed"], "smoke save")
    if not type_ref:
        type_ref = find_browser_ref(snap["parsed"], "textbox") or find_browser_ref(
            snap["parsed"], "input"
        )
    if not click_ref:
        raise SmokeError(f"browser snapshot has no Smoke Click ref: {extract_text(snap['parsed'])[-1500:]}")
    click_result = cua_call(
        "browser_click",
        {
            "target_id": target_id,
            "tab_id": tab_id,
            "ref": click_ref,
            "session": "lazyboy-smoke",
            "input_route": "dom_event",
        },
        timeout=60,
    )
    write_json(REPORT / f"browser-click-{iteration}.json", strip_heavy(click_result["parsed"] or click_result["stdout"]))
    time.sleep(0.4)
    after_click = cua_call(
        "get_browser_state",
        {
            "target_id": target_id,
            "tab_id": tab_id,
            "session": "lazyboy-smoke",
            "snapshot_format": "semantic_v2",
            "include_screenshot": False,
        },
        timeout=90,
    )
    write_json(REPORT / f"browser-after-click-{iteration}.json", strip_heavy(after_click["parsed"]))
    click_text = json.dumps(strip_heavy(after_click["parsed"]), default=str)
    if "clicked-ok" not in click_text:
        raise SmokeError(f"browser click did not change page text to clicked-ok: {click_text[-1500:]}")
    type_ref = find_browser_ref(after_click["parsed"], "smoke entry") or find_browser_ref(
        after_click["parsed"], "textbox"
    )
    save_ref = find_browser_ref(after_click["parsed"], "smoke save")
    if not type_ref:
        raise SmokeError("browser snapshot has no Smoke Entry ref")
    cua_call(
        "browser_type",
        {
            "target_id": target_id,
            "tab_id": tab_id,
            "ref": type_ref,
            "text": TYPED_TEXT,
            "replace": True,
            "session": "lazyboy-smoke",
        },
        timeout=60,
    )
    if not save_ref:
        raise SmokeError("browser snapshot has no Smoke Save ref")
    cua_call(
        "browser_click",
        {
            "target_id": target_id,
            "tab_id": tab_id,
            "ref": save_ref,
            "session": "lazyboy-smoke",
            "input_route": "dom_event",
        },
        timeout=60,
    )
    time.sleep(0.4)
    after_type = cua_call(
        "get_browser_state",
        {
            "target_id": target_id,
            "tab_id": tab_id,
            "session": "lazyboy-smoke",
            "snapshot_format": "semantic_v2",
            "include_screenshot": False,
        },
        timeout=90,
    )
    write_json(REPORT / f"browser-after-type-{iteration}.json", strip_heavy(after_type["parsed"]))
    type_text = json.dumps(strip_heavy(after_type["parsed"]), default=str)
    if f"typed:{TYPED_TEXT}" not in type_text:
        raise SmokeError(f"browser type did not land in the page: {type_text[-1500:]}")
    return {
        "pid": pid,
        "window_id": window_id,
        "target_id": target_id,
        "tab_id": tab_id,
        "prepared": prepared,
        "click_ref": click_ref,
    }


def novnc_ok() -> None:
    proc = run(
        [
            "python3",
            "-c",
            "import socket; s=socket.create_connection(('127.0.0.1',6080),2); s.close()",
        ],
        timeout=10,
    )
    if proc.returncode != 0:
        raise SmokeError("noVNC port 6080 is not accepting connections")


def extra_cua_processes() -> list[str]:
    proc = run(["ps", "-eo", "pid,cmd"], timeout=10)
    lines = []
    for line in (proc.stdout or "").splitlines():
        if "cua-driver" in line and "serve" not in line and "cua-smoke" not in line:
            if "cua-driver serve" in line:
                continue
            if str(os.getpid()) in line.split()[:1]:
                continue
            lines.append(line.strip())
    return lines


def one_iteration(iteration: int) -> dict[str, Any]:
    log(f"iteration {iteration}: screenshot")
    desktop_screenshot()
    log(f"iteration {iteration}: windows")
    windows = list_windows()
    if not windows:
        raise SmokeError("list_windows returned no windows")
    write_json(REPORT / f"windows-{iteration}.json", windows)
    log(f"iteration {iteration}: native click/type")
    native_round(iteration)
    log(f"iteration {iteration}: browser")
    browser = browser_round(iteration)
    log(f"iteration {iteration}: noVNC")
    novnc_ok()
    return {"windows": len(windows), "browser": browser}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeat", type=int, default=10)
    parser.add_argument("--ready-timeout", type=int, default=90)
    args = parser.parse_args()
    REPORT.mkdir(parents=True, exist_ok=True)
    summary: dict[str, Any] = {
        "repeat": args.repeat,
        "passed": 0,
        "failed": 0,
        "iterations": [],
        "ok": False,
    }
    fixture: subprocess.Popen[bytes] | None = None
    try:
        wait_ready(args.ready_timeout)
        fixture = start_fixture_server()
        diagnostics = collect_diagnostics()
        summary["diagnostics"] = {
            "version": diagnostics["version"],
            "doctor_code": diagnostics["doctor_code"],
            "status": diagnostics["status"],
        }
        for iteration in range(1, args.repeat + 1):
            try:
                detail = one_iteration(iteration)
                summary["passed"] += 1
                summary["iterations"].append({"n": iteration, "ok": True, **detail})
                log(f"iteration {iteration}: ok")
            except Exception as error:  # noqa: BLE001 — smoke must record any failure
                summary["failed"] += 1
                summary["iterations"].append(
                    {"n": iteration, "ok": False, "error": str(error)}
                )
                summary["error"] = str(error)
                log(f"iteration {iteration}: FAIL {error}")
                raise
        leftovers = extra_cua_processes()
        summary["leftover_cua_processes"] = leftovers
        if leftovers:
            raise SmokeError(f"extra cua-driver processes: {leftovers}")
        summary["ok"] = True
        log(f"passed {args.repeat}/{args.repeat}")
        return 0
    except subprocess.TimeoutExpired as error:
        summary["error"] = f"timeout: {error}"
        log(summary["error"])
        return 1
    except SmokeError as error:
        summary["error"] = str(error)
        log(f"FAIL {error}")
        return 1
    finally:
        write_json(REPORT / "result.json", summary)
        kill_matching("lazyboy-cua-smoke-gtk")
        if fixture is not None:
            fixture.kill()
        kill_matching(f"http.server {FIXTURE_PORT}")


if __name__ == "__main__":
    sys.exit(main())
