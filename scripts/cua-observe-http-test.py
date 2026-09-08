#!/usr/bin/env python3
"""Regression: a POST body + Connection: close must not truncate screenshots.

Run inside the computer image, as its desktop user, with LAZYBOY_CONTROL_TOKEN.
Uses the real controller/Cua and a local, dense screenshot fixture.
"""
import base64
import http.server
import importlib.machinery
import importlib.util
import json
import statistics
import threading
import time

loader = importlib.machinery.SourceFileLoader("adapter", "/usr/local/bin/lazyboy-cua-adapter-test")
spec = importlib.util.spec_from_loader(loader.name, loader)
adapter = importlib.util.module_from_spec(spec)
loader.exec_module(adapter)
HTML = b'''<!doctype html><title>Observe transport fixture</title>
<h1>Screenshot transport regression</h1><canvas width="800" height="400"></canvas>
<script>const c=document.querySelector('canvas'),x=c.getContext('2d'),p=x.createImageData(800,400);
let seed=73;for(let i=0;i<p.data.length;i+=4){for(let k=0;k<3;k++){seed=(Math.imul(seed,1664525)+1013904223)>>>0;p.data[i+k]=seed>>>24;}p.data[i+3]=255;}x.putImageData(p,0,0);</script>'''

class Fixture(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(HTML)))
        self.end_headers()
        self.wfile.write(HTML)

    def log_message(self, *_):
        pass

server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
threading.Thread(target=server.serve_forever, daemon=True).start()
try:
    adapter.wait_health()
    adapter.api("/browser", {"action": "snapshot", "ensure": True})
    adapter.api("/browser", {"action": "navigate", "ensure": True,
                           "url": f"http://127.0.0.1:{server.server_port}/"})
    timings = []
    sizes = []
    # urllib sends Connection: close. A nonempty request body reproduces the
    # old handler's unread-body reset; do not add retries that would mask it.
    for _ in range(20):
        before = time.monotonic()
        observation = adapter.observe()
        png = base64.b64decode(observation["png_base64"], validate=True)
        assert len(png) > 200_000, "fixture must exercise a large response"
        sizes.append(len(png))
        timings.append(round((time.monotonic() - before) * 1000))
    print(json.dumps({"passed": len(timings), "minPngBytes": min(sizes),
                      "medianMs": statistics.median(timings), "maxMs": max(timings)}))
finally:
    server.shutdown()
    server.server_close()
