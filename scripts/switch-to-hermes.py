#!/usr/bin/env python3
"""Stop this workspace's old runtimes, then start and verify LazyBoy Hermes."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time
from urllib.error import URLError
from urllib.parse import urlparse
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
OLD = ROOT.parent / "LazyBoy2"
DATA = Path(os.environ.get("LAZYHERMES_DATA_DIR", str(ROOT / ".data"))).resolve()
PORT = int(os.environ.get("LAZYHERMES_PORT", "8790"))


def run(args, *, capture=False, timeout=60, cwd=ROOT, env=None):
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=capture, timeout=timeout, env=env)
    if result.returncode:
        detail = (result.stderr or result.stdout or "").strip() if capture else ""
        raise RuntimeError(f"指令失敗：{' '.join(map(str, args))}\n{detail}")
    return result.stdout if capture else ""


def container_owner(item):
    """Names alone cannot select unrelated computers or Compose projects."""
    name = item.get("Name", "").lstrip("/")
    config = item.get("Config", {})
    labels = config.get("Labels") or {}
    working = labels.get("com.docker.compose.project.working_dir", "")
    if working and Path(working).is_relative_to(OLD): return "old"
    if working and Path(working).is_relative_to(ROOT): return "new-classifier"
    image = config.get("Image", "").split(":")[0]
    if (name == "lazyboy-box" or name.startswith("lazyboy-box-")) and image.rsplit("/", 1)[-1] == "lazyboy-box":
        return "old"
    if labels.get("lazyhermes.managed") == "true" and name == "lazyhermes-bot-" + labels.get("lazyhermes.bot", ""):
        homes = [m.get("Source", "") for m in item.get("Mounts", []) if m.get("Destination") == "/hermes"]
        if any(source and Path(source).is_relative_to(DATA / "agents") for source in homes): return "new-bot"
    return None


def runtime_owner(cwd, exe, args):
    """Match actual executable arguments, never arbitrary command substrings."""
    if not args: return None
    cwd, exe = Path(cwd), Path(exe)
    if exe.name == "lazyboy" and (exe.is_relative_to(OLD / "target") or cwd.is_relative_to(OLD)):
        return "old"
    if cwd.is_relative_to(OLD / "routing") or cwd.is_relative_to(OLD / "laya"):
        if any(Path(a).name == "uvicorn" for a in args) and any(app in args for app in ("server:app", "gateway:app")): return "old"
    for root, owner in ((OLD, "old"), (ROOT, "new")):
        if not cwd.is_relative_to(root): continue
        if owner == "new" and "server.serve" in args and exe.name.startswith(("python", "uv")): return owner
        if exe.name in {"node", "npm"} or args[0].startswith("npm "):
            if args[0].startswith("npm ") and " dev" in args[0]: return owner
            if cwd.is_relative_to(root / "web") and ("dev" in args or any("/vite/" in a or Path(a).name == "vite" for a in args)): return owner
    return None


def persistent_computer(item):
    agent = (item.get("Config", {}).get("Labels") or {}).get("lazyhermes.bot", "")
    mounts = {m.get("Destination"): m for m in item.get("Mounts", [])}
    for target, folder in (("/hermes", "hermes"), ("/home/box", "home"), ("/workspace", "workspace")):
        mount = mounts.get(target, {})
        if mount.get("Type") != "bind" or Path(mount.get("Source", "")).resolve() != DATA / "agents" / agent / folder:
            return False
    return True


def processes(proc=Path("/proc")):
    rows = {}
    for directory in proc.iterdir():
        if not directory.name.isdecimal(): continue
        try:
            stat = (directory / "stat").read_text().rsplit(")", 1)[1].split()
            args = [a.decode(errors="replace") for a in (directory / "cmdline").read_bytes().split(b"\0") if a]
            # /proc/exe remains valid after a rebuild unlinks its old inode.
            # Resolving that symlink strictly skips precisely the old daemon.
            cwd = Path(os.readlink(directory / "cwd").removesuffix(" (deleted)")).resolve()
            exe = Path(os.readlink(directory / "exe").removesuffix(" (deleted)"))
            pid = int(directory.name)
            rows[pid] = {"pid": pid, "parent": int(stat[1]), "stamp": stat[19], "state": stat[0], "owner": runtime_owner(cwd, exe, args)}
        except (OSError, ValueError, IndexError): pass
    # Also close browsers, MCPs and shell children launched by these runtimes.
    changed = True
    while changed:
        changed = False
        for row in rows.values():
            parent = rows.get(row["parent"], {})
            if not row["owner"] and parent.get("owner"):
                row["owner"] = parent["owner"]; changed = True
    return rows


def stop_processes(rows):
    selected = {pid: row for pid, row in rows.items() if row["owner"] and pid != os.getpid()}
    units = set()
    for pid in selected:
        try: cgroups = (Path("/proc") / str(pid) / "cgroup").read_text()
        except FileNotFoundError: continue
        for line in cgroups.splitlines():
            for part in line.split("/"):
                if part.endswith(".service") and any(name in part.lower() for name in ("lazyboy", "lazyhermes")):
                    units.add((part, "/user.slice/" in line))
    for unit, user in units: run(["systemctl", *(["--user"] if user else []), "stop", unit])
    # Give the product processes time to persist interrupted work first.
    roots = {pid: row for pid, row in selected.items() if row["parent"] not in selected}
    def terminate(targets, sig=signal.SIGTERM):
        current = processes()
        for pid, row in targets.items():
            if current.get(pid, {}).get("stamp") != row["stamp"]: continue
            try: os.kill(pid, sig)
            except ProcessLookupError: pass
    terminate(roots)
    started = time.monotonic()
    deadline = started + 40
    remaining = selected
    while time.monotonic() < deadline:
        current = processes()
        remaining = {pid: row for pid, row in selected.items() if current.get(pid, {}).get("stamp") == row["stamp"] and current[pid]["state"] != "Z"}
        if not remaining: return
        if time.monotonic() - started >= 5 or not any(pid in remaining for pid in roots): terminate(remaining)
        time.sleep(.2)
    # Stop all requested runtimes, including unresponsive descendants. Identity
    # is checked again immediately before signalling, so reused PIDs are spared.
    terminate(remaining, signal.SIGKILL)
    for _ in range(25):
        current = processes()
        if not any(current.get(pid, {}).get("stamp") == row["stamp"] and current[pid]["state"] != "Z" for pid, row in remaining.items()): return
        time.sleep(.2)
    raise RuntimeError("仍有舊程序未結束；尚未啟動新版。")


def listeners(proc=Path("/proc")):
    result = []
    for name, family in (("tcp", socket.AF_INET), ("tcp6", socket.AF_INET6)):
        path = proc / "net" / name
        if not path.exists(): continue
        for line in path.read_text().splitlines()[1:]:
            fields = line.split()
            if len(fields) < 10 or fields[3] != "0A": continue
            address, port = fields[1].split(":")
            raw = bytes.fromhex(address)
            raw = raw[::-1] if family == socket.AF_INET else b"".join(raw[i:i+4][::-1] for i in range(0, 16, 4))
            result.append({"address":socket.inet_ntop(family, raw), "port":int(port, 16), "inode":fields[9]})
    if not (proc / "net/tcp").exists(): raise RuntimeError("無法讀取主機監聽狀態，尚未確認切換完成。")
    return result


def verify_old_stopped():
    old_ports = {8787, 5173}
    active = [row for row in listeners() if row["port"] in old_ports]
    if active or any(row["owner"] == "old" for row in processes().values()):
        detail = ", ".join(f"{row['address']}:{row['port']}" for row in active)
        raise RuntimeError("舊版仍有程序或連接埠在運行：" + detail + "。尚未啟動新版；不會停止無法確認屬於本專案的程序。")


def verify_public_listener(proc=Path("/proc")):
    pid = int((DATA / "service.lock").read_text())
    fds = proc / str(pid) / "fd"
    inodes = set()
    for fd in fds.iterdir():
        try: target = os.readlink(fd)
        except FileNotFoundError: continue
        if target.startswith("socket:["): inodes.add(target[8:-1])
    if not any(row["address"] == "0.0.0.0" and row["port"] == PORT and row["inode"] in inodes for row in listeners(proc)):
        raise RuntimeError(f"新版尚未監聽 0.0.0.0:{PORT}；不能宣稱已對外開放。")


def start_product():
    run(["scripts/start-background.sh"], timeout=60, env={**os.environ, "LAZYHERMES_BIND":"0.0.0.0"})
    verify_public_listener()


def verify_process_visibility(rows):
    lock = DATA / "service.lock"
    if not lock.exists(): return
    with lock.open("r") as handle:
        try: fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            pid = int(handle.read())
            if rows.get(pid, {}).get("owner") != "new":
                raise RuntimeError("主機 Hermes 正在運行，但本工作階段看不到它的程序。請在主機終端執行，不會把隔離環境內的空清單當成已關閉。")


def stop_old_services():
    current = processes()
    verify_process_visibility(current)
    rows = {pid: row for pid, row in current.items() if row["owner"] == "old"}
    for pid in rows: print(f"停止舊版程序：{pid}", flush=True)
    stop_processes(rows)
    for item in containers():
        if container_owner(item) == "old" and item["State"]["Running"]:
            print("停止舊版容器：" + item["Name"].lstrip("/"), flush=True)
            run(["docker", "stop", "-t", "20", item["Id"]], timeout=45)
    verify_old_stopped()
    if any(container_owner(item) == "old" and item["State"]["Running"] for item in containers()):
        raise RuntimeError("仍有舊版容器運行，尚未完成停止。")
    print("舊版程序、8787／5173 及容器已停止；新版服務維持運行。", flush=True)


def api(path, body=None, *, method=None):
    headers = {}
    token = os.environ.get("LAZYHERMES_ACCESS_TOKEN")
    if not token and (DATA / "access-token").exists(): token = (DATA / "access-token").read_text().strip()
    if token: headers["Authorization"] = "Bearer " + token
    if body is not None: headers["Content-Type"] = "application/json"
    request = Request(f"http://127.0.0.1:{PORT}" + path, data=json.dumps(body).encode() if body is not None else None, headers=headers, method=method)
    with urlopen(request, timeout=120 if body is not None else 3) as response: return json.load(response)


def containers():
    ids = run(["docker", "ps", "-aq"], capture=True).split()
    return json.loads(run(["docker", "inspect", *ids], capture=True)) if ids else []


def wait_classifier():
    url = f"http://127.0.0.1:{int(os.environ.get('LAZYHERMES_CLASSIFIER_PORT', '8778'))}/health"
    deadline = time.monotonic() + 600
    while time.monotonic() < deadline:
        try:
            with urlopen(url, timeout=3) as response:
                if json.load(response).get("ready"): return
        except (URLError, OSError, ValueError): pass
        time.sleep(2)
    raise RuntimeError("新版已啟動，但分類器暖機尚未完成。請查看 scripts/routing.sh status 與容器日誌。")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="僅檢查權限並顯示切換範圍，不啟停服務")
    mode.add_argument("--stop-old", action="store_true", help="只停止舊版，不建置或重啟目前的新版")
    args = parser.parse_args()
    # Abort before touching PID files, processes, containers or product data.
    run(["docker", "info", "--format", "{{.ServerVersion}}"], capture=True)
    if args.stop_old:
        stop_old_services(); return 0
    run([sys.executable, "scripts/doctor.py"], capture=True)
    selected = [(item, container_owner(item)) for item in containers()]
    selected = [(item, owner) for item, owner in selected if owner]
    rows = processes()
    verify_process_visibility(rows)
    for pid, row in rows.items():
        if row["owner"]: print(f"程序 {row['owner']}: {pid}", flush=True)
    for item, owner in selected: print(f"容器 {owner}: {item['Name'].lstrip('/')}", flush=True)
    if args.check:
        print("檢查完成；尚未啟停任何服務。", flush=True); return 0
    if any(owner == "new-bot" and not persistent_computer(item) for item, owner in selected):
        raise RuntimeError("Hermes 電腦的持久資料掛載不符，尚未停止服務；不會刪除這些容器。")
    settings_path = DATA / "settings.json"
    routing = json.loads(settings_path.read_text()).get("routing", {}) if settings_path.exists() else {}
    classifier = bool(routing.get("semantic") or routing.get("laya") or any(
        item["State"]["Running"] and (owner == "new-classifier" or (owner == "old" and (item["Config"].get("Labels") or {}).get("com.docker.compose.service") in {"classifier", "openvino"}))
        for item, owner in selected))
    print("先建置新版；建置成功後才關閉舊版。", flush=True)
    run(["scripts/bootstrap.sh"], timeout=3600)
    if classifier:
        run(["docker", "compose", "-p", "lazyhermes-routing", "-f", str(ROOT / "routing/compose.yaml"), "build"], timeout=1800)
    print("停止舊版及目前 Hermes 服務。", flush=True)
    stop_processes(processes())
    # A task may have created another computer while images were building.
    selected = [(item, container_owner(item)) for item in containers()]
    selected = [(item, owner) for item, owner in selected if owner]
    for item, _ in selected:
        if item["State"]["Running"]: run(["docker", "stop", "-t", "20", item["Id"]], timeout=45)
    verify_old_stopped()
    # New computers bind-mount all durable state. Recreate only their container,
    # so an old image cannot silently survive the switch. Old volumes stay put.
    running_bots = []
    for item, owner in selected:
        if owner != "new-bot": continue
        if not persistent_computer(item): raise RuntimeError("電腦掛載已變更，保留容器與資料：" + item["Name"])
        if item["State"]["Running"]: running_bots.append(item["Config"]["Labels"]["lazyhermes.bot"])
        run(["docker", "rm", item["Id"]])
    if classifier: run(["scripts/routing.sh", "start"], timeout=1800)
    start_product()
    if api("/api/health").get("engine") != "hermes" or not api("/api/settings/routing/health").get("supported"):
        raise RuntimeError("服務已啟動，但尚未通過新版分流檢查。")
    old_classifier = urlparse(routing.get("classifier_url", ""))
    if classifier and old_classifier.hostname in {"localhost", "127.0.0.1", "::1"} and old_classifier.port in {8766, 8768, 8770}:
        target = f"http://127.0.0.1:{int(os.environ.get('LAZYHERMES_CLASSIFIER_PORT', '8778'))}"
        api("/api/settings", {"routing":{"classifier_url":target}}, method="PUT")
    for agent in running_bots:
        if not api(f"/api/agents/{agent}/computer", {"action": "start"}).get("ready"):
            raise RuntimeError("新版 Bot 電腦尚未就緒：" + agent)
    if classifier:
        print("等待新版分類器完成暖機…", flush=True)
        wait_classifier()
    # Inspect again, rather than treating successful stop commands as proof.
    remaining_ids = run(["docker", "ps", "-q"], capture=True).split()
    remaining = json.loads(run(["docker", "inspect", *remaining_ids], capture=True)) if remaining_ids else []
    if any(container_owner(item) == "old" for item in remaining) or any(row["owner"] == "old" for row in processes().values()):
        raise RuntimeError("新版已啟動，但仍偵測到舊版服務。")
    verify_old_stopped()
    verify_public_listener()
    print(f"切換完成：監聽 0.0.0.0:{PORT}，其他裝置使用 http://<主機 IP>:{PORT}；舊版已停止，對話、檔案與登入資料保留。", flush=True)
    return 0


if __name__ == "__main__":
    try: raise SystemExit(main())
    except (RuntimeError, OSError, subprocess.TimeoutExpired, URLError, ValueError) as error:
        print(str(error), file=sys.stderr); raise SystemExit(1)
