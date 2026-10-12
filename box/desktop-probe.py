#!/usr/bin/env python3
"""Check a complete, shared RFB handshake through the actual noVNC bridge.

Reading only the banner and disconnecting counts as a failed authentication
in TigerVNC. Repeated health checks must never blacklist the viewer itself.
Only ServerInit means the desktop can accept a viewer; no input is sent.
"""
import argparse
import base64
import hashlib
import os
import socket
import struct
import time
import urllib.request


class Stream:
    def __init__(self, sock, deadline, websocket=False):
        self.sock, self.deadline, self.websocket = sock, deadline, websocket
        self.pending = bytearray()

    def raw(self, size):
        result = bytearray()
        while len(result) < size:
            remaining = self.deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("desktop handshake timed out")
            self.sock.settimeout(remaining)
            chunk = self.sock.recv(size - len(result))
            if not chunk:
                raise ConnectionError("desktop closed the handshake")
            result.extend(chunk)
        return bytes(result)

    def send(self, data, opcode=2):
        if self.websocket:
            mask = os.urandom(4)
            header = bytes([0x80 | opcode, 0x80 | len(data)])
            data = header + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data))
        self.sock.settimeout(max(0.001, self.deadline - time.monotonic()))
        self.sock.sendall(data)

    def read(self, size):
        if not self.websocket:
            return self.raw(size)
        while len(self.pending) < size:
            first, second = self.raw(2)
            opcode, length = first & 15, second & 127
            if second & 128:
                raise ValueError("server sent a masked WebSocket frame")
            if length == 126:
                length = struct.unpack("!H", self.raw(2))[0]
            elif length == 127:
                length = struct.unpack("!Q", self.raw(8))[0]
            if length > 65536:
                raise ValueError("oversized desktop handshake")
            if opcode >= 8 and length > 125:
                raise ValueError("oversized WebSocket control frame")
            payload = self.raw(length)
            if opcode in (0, 2):
                self.pending.extend(payload)
            elif opcode == 9:
                self.send(payload, 10)
            elif opcode != 10:
                raise ConnectionError("WebSocket closed or returned non-RFB data")
        result = bytes(self.pending[:size])
        del self.pending[:size]
        return result


def websocket_stream(sock, deadline):
    stream = Stream(sock, deadline)
    key = base64.b64encode(os.urandom(16)).decode()
    stream.send(("GET /websockify HTTP/1.1\r\nHost: 127.0.0.1:6080\r\n"
                 "Upgrade: websocket\r\nConnection: Upgrade\r\n"
                 f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
                 "Sec-WebSocket-Protocol: binary\r\n\r\n").encode())
    response = bytearray()
    while not response.endswith(b"\r\n\r\n"):
        if len(response) >= 4096:
            raise ValueError("oversized WebSocket upgrade")
        response.extend(stream.raw(1))
    lines = response.decode("latin1").split("\r\n")
    if lines[0].split()[1] != "101":
        raise ConnectionError("noVNC bridge refused the WebSocket upgrade")
    headers = {name.lower(): value.strip() for line in lines[1:] if ":" in line
               for name, value in [line.split(":", 1)]}
    accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
    if headers.get("sec-websocket-accept") != accept:
        raise ValueError("invalid WebSocket upgrade")
    stream.websocket = True
    return stream


def handshake(stream):
    banner = stream.read(12)
    if banner not in (b"RFB 003.003\n", b"RFB 003.007\n", b"RFB 003.008\n"):
        raise ValueError("unsupported RFB protocol")
    stream.send(banner)
    if banner == b"RFB 003.003\n":
        if stream.read(4) != b"\0\0\0\1":
            raise ConnectionError("VNC refused the connection")
    else:
        count = stream.read(1)[0]
        if not count or 1 not in stream.read(count):
            raise ConnectionError("VNC refused passwordless authentication")
        stream.send(b"\1")
        if banner == b"RFB 003.008\n" and stream.read(4) != b"\0\0\0\0":
            raise ConnectionError("VNC authentication failed")
    stream.send(b"\1")  # Shared ClientInit: retain all existing viewers.
    width, height = struct.unpack("!HH", stream.read(4))
    stream.read(16)  # Pixel format.
    name_length = struct.unpack("!I", stream.read(4))[0]
    if not width or not height or name_length > 4096:
        raise ValueError("invalid VNC ServerInit")
    stream.read(name_length)


def probe(vnc_host=None):
    deadline = time.monotonic() + 2
    if vnc_host is None:
        with urllib.request.urlopen("http://127.0.0.1:6080/vnc.html", timeout=1) as response:
            if response.status != 200:
                raise ConnectionError("noVNC page unavailable")
    with socket.create_connection((vnc_host or "127.0.0.1", 5900 if vnc_host else 6080), timeout=1) as sock:
        stream = Stream(sock, deadline) if vnc_host else websocket_stream(sock, deadline)
        handshake(stream)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--vnc-host")
    args = parser.parse_args()
    probe(args.vnc_host)
