#!/usr/bin/env python3
"""Read-only install checks. Never opens product data or starts containers."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def command(args):
    try:
        result = subprocess.run(args, capture_output=True, text=True, timeout=15)
        return result.returncode == 0, (result.stdout or result.stderr).strip()[:500]
    except (OSError, subprocess.TimeoutExpired) as error:
        return False, str(error)


def inspect(*, static=False, ready=False):
    checks = []
    for name in ("git", "uv", "node", "npm", "docker"):
        executable = shutil.which(name)
        ok, detail = command([executable, "--version"]) if executable else (False, "未安裝")
        if name == "node" and ok:
            version = re.search(r"v(\d+)", detail)
            ok = bool(version and int(version.group(1)) >= 20)
        checks.append({"name":name,"passed":ok,"detail":detail})
    for name in ("HERMES_VERSION", "uv.lock", "web/package-lock.json", "box/playwright/package.json"):
        path = ROOT/name
        checks.append({"name":name,"passed":path.exists(),"detail":"存在" if path.exists() else "缺少檔案"})
    if not static:
        ok, detail = command(["docker", "info", "--format", "{{.ServerVersion}}"])
        checks.append({"name":"Docker daemon","passed":ok,"detail":detail})
        # The images use BuildKit features (RUN --mount, COPY --chmod) that need the buildx plugin.
        ok, detail = command(["docker", "buildx", "version"])
        checks.append({"name":"Docker Buildx","passed":ok,"detail":detail})
        ok, detail = command(["docker", "compose", "version"])
        checks.append({"name":"Docker Compose","passed":ok,"optional":True,"detail":detail if ok else "選用：在 Docker 中執行（make up）與語意路由（scripts/routing.sh）需要"})
    if ready:
        expected = (ROOT/"HERMES_VERSION").read_text().strip()
        ok, revision = command(["git", "-C", str(ROOT/"vendor/hermes-agent"), "rev-parse", "HEAD"])
        checks.append({"name":"Pinned Hermes","passed":ok and revision==expected,"detail":revision})
        for name in (".venv/bin/python", "web/dist/index.html"):
            checks.append({"name":name,"passed":(ROOT/name).exists(),"detail":"安裝產物"})
        if not static:
            for image in ("lazyhermes-desktop:local", os.environ.get("LAZYHERMES_IMAGE", "lazyhermes-bot:local")):
                ok, detail = command(["docker", "image", "inspect", image, "--format", "{{.Id}}"])
                checks.append({"name":image,"passed":ok,"detail":detail})
    return {"passed":all(c["passed"] or c.get("optional") for c in checks),"static_only":static,"checks":checks}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--static",action="store_true",help="檢查檔案與程式版本，不連線 Docker daemon")
    parser.add_argument("--ready",action="store_true",help="另檢查已安裝產物與 Docker 映像")
    parser.add_argument("--json",action="store_true")
    args=parser.parse_args()
    report=inspect(static=args.static,ready=args.ready)
    if args.json: print(json.dumps(report,ensure_ascii=False,indent=2))
    else:
        for check in report["checks"]:
            print(("PASS" if check["passed"] else "WARN" if check.get("optional") else "FAIL")+" "+check["name"]+": "+check["detail"])
    return 0 if report["passed"] else 1


if __name__=="__main__": raise SystemExit(main())
