import argparse
import os
from pathlib import Path
import signal
import sys
import time
import httpx

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from server.config import DATA, HTTPS_PORT, PORT

parser = argparse.ArgumentParser(); parser.add_argument("action", choices=["wait", "stop"]); args = parser.parse_args()
if args.action == "wait":
    for attempt in range(75):
        try:
            response = httpx.get(f"http://127.0.0.1:{PORT}/api/health", timeout=1)
            if response.is_success and response.json().get("engine") == "hermes":
                print(f"LazyBoy 已啟動：http://localhost:{PORT}")
                if HTTPS_PORT: print(f"手機與其他裝置用 HTTPS 連線（port {HTTPS_PORT}）：網址與登入 QR code 在「設定 → 其他裝置與 App」")
                raise SystemExit(0)
        except httpx.HTTPError: pass
        time.sleep(.2)
    raise SystemExit("服務尚未啟動，請查看 .run/server.log")
lock = DATA / "service.lock"
if not lock.exists(): raise SystemExit("沒有正在執行的服務")
try:
    pid = int(lock.read_text())
    process = Path("/proc") / str(pid)
    command = (process / "cmdline").read_bytes()
    if b"server.serve" not in command or (process / "cwd").resolve() != ROOT:
        raise SystemExit("記錄的 PID 不是本專案服務，沒有停止其他程式")
    os.kill(pid, signal.SIGTERM)
    for attempt in range(180):
        if not process.exists(): print("LazyBoy 已停止；Bot 的檔案與登入已保留。"); break
        time.sleep(.2)
    else: raise SystemExit("仍在停止中；請查看 .run/server.log，沒有強制終止")
except (ValueError, ProcessLookupError, FileNotFoundError): raise SystemExit("服務已停止")
