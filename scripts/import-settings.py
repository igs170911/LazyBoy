"""Import provider settings only, never previous conversations or browser profiles."""
import argparse
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from server.config import DATA, Settings
from server.store import Store

parser = argparse.ArgumentParser()
parser.add_argument("source", type=Path)
parser.add_argument("--force", action="store_true")
args = parser.parse_args()
target = DATA / "settings.json"
if target.exists() and not args.force:
    raise SystemExit("已有模型設定；若確定要覆蓋，請加 --force")
raw = json.loads(args.source.read_text())
providers = raw.get("providers", {})
if not isinstance(providers, dict) or not providers: raise SystemExit("找不到供應商設定")
config = {"active": raw.get("active", next(iter(providers))), "model": raw.get("model", ""), "providers": providers}
settings = Settings(Store(DATA)); settings.save(config)
print(f"已匯入 {len(providers)} 個供應商到 {target}；沒有輸出或分享 API key。")
