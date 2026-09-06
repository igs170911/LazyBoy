#!/usr/bin/env python3
"""Rotate only LazyBoy service logs; preserve open file descriptors (copytruncate)."""
import fcntl
import os
from pathlib import Path
import stat
import time

MAX_BYTES = 5 * 1024 * 1024
BACKUPS = 3


def rotate(root: Path, limit: int = MAX_BYTES):
    for path in root.glob('*.log'):
        try:
            fd = os.open(path, os.O_RDWR | os.O_NOFOLLOW)
            with os.fdopen(fd, 'r+b') as source:
                info = os.fstat(source.fileno())
                if not stat.S_ISREG(info.st_mode) or info.st_size <= limit:
                    continue
                # Keep a bounded tail even after an unusually large burst.
                source.seek(-limit, os.SEEK_END)
                tail = source.read(limit)
                for n in range(BACKUPS - 1, 0, -1):
                    previous = Path(f'{path}.{n}')
                    if previous.exists() or previous.is_symlink():
                        os.replace(previous, Path(f'{path}.{n + 1}'))
                backup_fd = os.open(f'{path}.1', os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o600)
                with os.fdopen(backup_fd, 'wb') as backup:
                    backup.write(tail)
                source.truncate(0)
        except (OSError, ValueError) as error:
            print(f'log rotation skipped {path.name}: {error}', flush=True)


def main():
    root = Path('/tmp/lazyboy')
    root.mkdir(exist_ok=True)
    with (root / 'log-rotation.lock').open('w') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        while True:
            rotate(root)
            time.sleep(60)


if __name__ == '__main__':
    main()
