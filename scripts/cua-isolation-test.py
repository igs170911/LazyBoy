#!/usr/bin/env python3
"""Verify Cua native handles cannot cross two LazyBoy display sessions."""
import importlib.machinery
import importlib.util
import json
import os
import subprocess
import time
from pathlib import Path

loader = importlib.machinery.SourceFileLoader("adapter", "/usr/local/bin/lazyboy-cua-adapter-test")
spec = importlib.util.spec_from_loader(loader.name, loader)
adapter = importlib.util.module_from_spec(spec)
loader.exec_module(adapter)


def main():
    subprocess.run(["lazyboy-screen", "ensure", "1"], check=True, timeout=60)
    adapter.smoke.REPORT.mkdir(parents=True, exist_ok=True)
    roots = [Path("/tmp/lazyboy/isolation-1"), Path("/tmp/lazyboy/isolation-2")]
    processes = []
    try:
        for number, root in enumerate(roots, 1):
            root.mkdir(exist_ok=True)
            (root / "cua-smoke-clicked").unlink(missing_ok=True)
            env = os.environ.copy()
            env.update(DISPLAY=f":{number}", LAZYBOY_SMOKE_ROOT=str(root),
                       LAZYBOY_SMOKE_TITLE=f"Isolation {number}",
                       DBUS_SESSION_BUS_ADDRESS=Path(f"/tmp/lazyboy/screen-{number}.dbus").read_text().strip())
            processes.append(subprocess.Popen(["/usr/local/bin/lazyboy-cua-smoke-gtk"], env=env))
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and not Path("/tmp/lazyboy/cua-2.sock").is_socket():
            time.sleep(0.2)
        time.sleep(1)
        pages = [adapter.api("/observe", {}, display=f":{n}") for n in (1, 2)]
        targets = [adapter.element(page, "Smoke Click")["selector"] for page in pages]
        assert targets[0] != targets[1]
        assert all(adapter.api("/controller/health", display=f":{n}")["healthy"] for n in (1, 2))
        body = lambda target: {"actions": [{"kind": "ref", "verb": "click", "refKind": "a11y", "target": target}], "observe": False, "settle_ms": 100}
        adapter.api("/act", body(targets[0]), display=":2", expect_error=True)
        assert not any((root / "cua-smoke-clicked").exists() for root in roots)
        adapter.api("/act", body(targets[0]), display=":1")
        assert (roots[0] / "cua-smoke-clicked").read_text().strip() == "clicked"
        assert not (roots[1] / "cua-smoke-clicked").exists()
        adapter.api("/act", body(targets[1]), display=":2")
        assert (roots[1] / "cua-smoke-clicked").read_text().strip() == "clicked"
        result = {"displays": 2, "cross_display_ref_rejected": True, "independent_clicks": True}
        (adapter.smoke.REPORT / "isolation.json").write_text(json.dumps(result, indent=2))
        print(json.dumps(result), flush=True)
    finally:
        for process in processes:
            process.terminate()
            process.wait(timeout=5)


if __name__ == "__main__":
    main()
