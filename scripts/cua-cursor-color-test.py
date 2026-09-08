#!/usr/bin/env python3
"""Verify selected colors in the actual VNC framebuffer, without VNC input."""
import importlib.machinery
import importlib.util
import struct
import socket
import subprocess
import time
import zlib
from pathlib import Path

loader = importlib.machinery.SourceFileLoader('adapter', '/usr/local/bin/lazyboy-cua-adapter-test')
spec = importlib.util.spec_from_loader(loader.name, loader)
adapter = importlib.util.module_from_spec(spec)
loader.exec_module(adapter)


def capture():
    with socket.create_connection(('127.0.0.1', 5900), 5) as stream:
        def read(size):
            data = bytearray()
            while len(data) < size:
                chunk = stream.recv(size - len(data))
                assert chunk, 'VNC closed before a complete framebuffer'
                data.extend(chunk)
            return bytes(data)
        assert read(12).startswith(b'RFB ')
        stream.sendall(b'RFB 003.008\n')
        assert 1 in read(read(1)[0]), 'disposable desktop must offer unauthenticated loopback VNC'
        stream.sendall(b'\x01')
        assert read(4) == b'\0' * 4
        stream.sendall(b'\x01')
        width, height = struct.unpack('>HH', read(4))
        read(16)
        read(struct.unpack('>I', read(4))[0])
        assert 0 < width <= 4096 and 0 < height <= 4096
        # Request raw pixels in RGBX byte order. Never send keys or pointer input.
        stream.sendall(b'\0' * 4 + struct.pack('>BBBBHHHBBBxxx', 32, 24, 0, 1, 255, 255, 255, 0, 8, 16))
        stream.sendall(struct.pack('>BBHi', 2, 0, 1, 0))
        stream.sendall(struct.pack('>BBHHHH', 3, 0, 0, 0, width, height))
        pixels = bytearray(width * height * 4)
        while True:
            kind = read(1)[0]
            if kind == 2:
                continue
            if kind == 3:
                read(3)
                read(struct.unpack('>I', read(4))[0])
                continue
            assert kind == 0, kind
            read(1)
            for _ in range(struct.unpack('>H', read(2))[0]):
                x, y, w, h, encoding = struct.unpack('>HHHHi', read(12))
                assert encoding == 0 and x + w <= width and y + h <= height
                rectangle = read(w * h * 4)
                for row in range(h):
                    start = ((y + row) * width + x) * 4
                    pixels[start:start + w * 4] = rectangle[row * w * 4:(row + 1) * w * 4]
            return width, height, pixels


def save_png(path, width, height, pixels):
    pixels[3::4] = b'\xff' * (width * height)
    rows = b''.join(b'\0' + pixels[y * width * 4:(y + 1) * width * 4] for y in range(height))
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    Path(path).write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


adapter.wait_health()
root = Path('/tmp/lazyboy')
files = [root / 'screen-1.agent-name', root / 'screen-1.agent-color', root / 'screen-2.agent-color']
previous = {path: path.read_bytes() if path.exists() else None for path in files}
name = '小幫手 Alice'
try:
    subprocess.run(['lazyboy-screen', 'ensure', '0', '', name, '#8B5CF6'], check=True)
    adapter.api('/browser', {'action': 'snapshot', 'ensure': True})
    adapter.api('/browser', {'action': 'navigate', 'url': 'about:blank', 'ensure': True})
    pid = (root / 'screen-1-cua.pid').read_text()
    for color, label in [('#8B5CF6', 'purple'), ('#22C55E', 'green'), ('#E11D48', 'pink')]:
        subprocess.run(['lazyboy-screen', 'color', '0', color], check=True)
        subprocess.run(['lazyboy-screen', 'color', '1', '#123456'], check=True)
        assert (root / 'screen-1-cua.pid').read_text() == pid, 'color change restarted Cua'
        assert files[0].read_text() == name, 'color change renamed the session'
        adapter.api('/act', {'actions': [{'kind': 'pointer', 'type': 'click', 'x': 640, 'y': 400}], 'observe': False, 'settle_ms': 100})
        expected = bytes.fromhex(color[1:])
        deadline = time.monotonic() + 5
        while True:
            width, height, pixels = capture()
            matches = sum(pixels[(y * width + x) * 4:(y * width + x) * 4 + 3] == expected
                          for y in range(380, 425) for x in range(620, 665))
            if matches >= 3:
                break
            assert time.monotonic() < deadline, (color, 'selected RGB absent from VNC cursor')
            time.sleep(0.1)
        save_png(f'/tmp/cua-cursor-{label}.png', width, height, pixels)
        print(f'VNC cursor {color}: {matches} exact RGB pixels, same session and daemon', flush=True)
    rejected = subprocess.run(['lazyboy-screen', 'color', '0', 'red'], capture_output=True)
    assert rejected.returncode != 0 and files[1].read_text() == '#E11D48'
finally:
    adapter.smoke.cua_call('end_session', {'session': name})
    for path, value in previous.items():
        if value is None:
            path.unlink(missing_ok=True)
        else:
            path.write_bytes(value)
print('Selected cursor colors, live updates, display isolation and invalid-color rejection passed')
