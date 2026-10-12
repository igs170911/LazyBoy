"""Flush Chromium before the container's init exits."""
import os, signal, time
from pathlib import Path
pids=[]
for proc in Path('/proc').iterdir():
    if not proc.name.isdigit():
        continue
    try:
        cmd=(proc/'cmdline').read_bytes().split(b'\0')
        if cmd and cmd[0].endswith(b'/chromium') and not any(arg.startswith(b'--type=') for arg in cmd):
            pid=int(proc.name); os.kill(pid,signal.SIGTERM); pids.append(pid)
    except (OSError, ValueError):
        pass
for _ in range(50):
    if not any(Path(f'/proc/{pid}').exists() for pid in pids):
        break
    time.sleep(.1)
