"""Use the reviewed upstream exporter; keep weights out of the repository."""
import hashlib
import os
from pathlib import Path
import runpy
import sys
import urllib.request

REVISION = "8a6e1328cce2460a0e5aa348ad465bb1b5821cd2"
URL = f"https://raw.githubusercontent.com/NandhaKishorM/laya/{REVISION}/scripts/export_onnx.py"
# Reviewed upstream code, pinned independently of package releases.
EXPECTED_SHA256 = "3dc6eac8971358ad5531c7124f2e631ddd4da6890a8df8a6237051e56b4594e2"
path = Path("/tmp/lazyboy-export-onnx.py")
data = urllib.request.urlopen(URL, timeout=30).read()
if hashlib.sha256(data).hexdigest() != EXPECTED_SHA256:
    raise RuntimeError("upstream exporter hash mismatch")
path.write_bytes(data)
import torch
torch.set_num_threads(int(os.getenv("LAYA_THREADS", "2")))
sys.argv = [str(path), "--model", os.getenv("LAYA_MODEL_PATH", "convaiinnovations/laya-multilingual"),
    "--output", os.getenv("LAYA_ONNX_PATH", "/models/laya.onnx")]
runpy.run_path(str(path), run_name="__main__")
