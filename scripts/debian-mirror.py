#!/usr/bin/env python3
"""Print the fastest Debian mirror for building the bot computer images.

DEBIAN_MIRROR in the environment is used as is. Otherwise every candidate is asked for the trixie package
index at once, the quickest to answer get a speed test one at a time, and the fastest wins. Measurements go to
stderr. stdout gets the main archive URL, then the same host's debian-security archive when it carries one
(Debian's Chromium ships through security updates, so that one matters too).
"""
import os
import sys
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor

OFFICIAL = "http://deb.debian.org/debian"
# The global CDN plus well-connected national mirrors from https://www.debian.org/mirror/list. Plain HTTP on
# purpose: the base image has no CA certificates yet, and apt verifies every package's signature anyway.
CANDIDATES = [
    OFFICIAL,
    "http://free.nchc.org.tw/debian", "http://ftp.tw.debian.org/debian", "http://debian.csie.ntu.edu.tw/debian",
    "http://ftp.jp.debian.org/debian", "http://ftp.kr.debian.org/debian", "http://ftp.hk.debian.org/debian",
    "http://mirrors.tuna.tsinghua.edu.cn/debian", "http://ftp.sg.debian.org/debian", "http://ftp.au.debian.org/debian",
    "http://ftp.us.debian.org/debian", "http://ftp.de.debian.org/debian", "http://ftp.uk.debian.org/debian",
]
SUITE = "trixie"  # box/Dockerfile's Debian release
PROBE = f"/dists/{SUITE}/main/binary-amd64/Packages.xz"
FINALISTS = 4
SAMPLE_BYTES = 4 << 20
SAMPLE_SECONDS = 3.0


def first_byte(mirror):
    """Seconds until the package index starts arriving, or None when the mirror does not answer."""
    started = time.monotonic()
    try:
        with urllib.request.urlopen(mirror + PROBE, timeout=2) as response:
            response.read(1)
        return time.monotonic() - started
    except OSError:
        return None


def throughput(mirror):
    """Bytes per second over the first few MB of the index. Run alone, so mirrors never share the line."""
    started, received = time.monotonic(), 0
    try:
        with urllib.request.urlopen(mirror + PROBE, timeout=3) as response:
            while received < SAMPLE_BYTES and time.monotonic() - started < SAMPLE_SECONDS:
                chunk = response.read(64 << 10)
                if not chunk:
                    break
                received += len(chunk)
    except OSError:
        return 0.0
    return received / max(time.monotonic() - started, 1e-3)


def security_archive(mirror):
    """The mirror host's debian-security archive, or None when it has none (deb.debian.org's is used then)."""
    if mirror == OFFICIAL or not mirror.endswith("/debian"):
        return None
    try:
        with urllib.request.urlopen(mirror + f"-security/dists/{SUITE}-security/Release", timeout=3) as response:
            response.read(1)
        return mirror + "-security"
    except OSError:
        return None


def choose(mirror):
    security = security_archive(mirror)
    print(f"使用鏡像站：{mirror}" + (f"，安全性更新：{security}" if security else ""), file=sys.stderr)
    print(mirror)
    if security:
        print(security)


def main():
    chosen = os.environ.get("DEBIAN_MIRROR", "").strip().rstrip("/")
    if chosen:
        print("使用 DEBIAN_MIRROR 指定的鏡像站", file=sys.stderr)
        choose(chosen)
        return
    print("正在測試 Debian 鏡像站速度…", file=sys.stderr)
    with ThreadPoolExecutor(len(CANDIDATES)) as pool:
        latency = dict(zip(CANDIDATES, pool.map(first_byte, CANDIDATES)))
    nearest = sorted((m for m in CANDIDATES if latency[m] is not None), key=latency.get)
    finalists = list(dict.fromkeys(nearest[:FINALISTS] + ([OFFICIAL] if OFFICIAL in nearest else [])))
    results = sorted(((throughput(m), m) for m in finalists), reverse=True)
    for speed, mirror in results:
        print(f"  {mirror.split('/')[2]:<30} {speed / 1e6:6.1f} MB/s", file=sys.stderr)
    choose(results[0][1] if results and results[0][0] > 0 else OFFICIAL)


if __name__ == "__main__":
    main()
