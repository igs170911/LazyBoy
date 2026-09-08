#!/usr/bin/env python3
"""Real terminal persistence/interrupt/reset via controld's Cua action route.
Run inside a disposable computer container. Files are test oracles, never an
agent-side output channel. Every application/input action goes through Cua.
"""
import base64
import importlib.machinery
import importlib.util
import time
import os
from pathlib import Path

loader = importlib.machinery.SourceFileLoader("adapter", "/usr/local/bin/lazyboy-cua-adapter-test")
spec = importlib.util.spec_from_loader(loader.name, loader)
adapter = importlib.util.module_from_spec(spec)
loader.exec_module(adapter)
adapter.wait_health()
root = Path('/tmp/lazyboy/cua-terminal-test')
root.mkdir(exist_ok=True)
title = 'LazyBoy terminal cua-persistence-' + str(time.monotonic_ns())
for filename in ['unicode.txt', 'persistence.txt', 'interrupt.txt', 'reset.txt', 'background.pid']:
    (root / filename).unlink(missing_ok=True)


def act(actions, name, settle=1000):
    result = adapter.api('/act', {'actions': actions, 'observe': True, 'settle_ms': settle})
    frame = base64.b64decode(result['png_base64'])
    assert frame.startswith(b'\x89PNG'), 'missing shared-desktop frame'
    (root / (name + '.png')).write_bytes(frame)
    return result


def key(value):
    return {'kind': 'key', 'key': value}


def encoded_command(value):
    encoded = ''.join('\\\\' if b == 92 else "\\'" if b == 39 else chr(b) if 32 <= b <= 126 else f'\\x{b:02x}' for b in value.encode())
    return "eval $'" + encoded + "'"


def command(value, name):
    value = encoded_command(value)
    return act([{'kind':'focus', 'title':title}, {'kind':'clipboard','text':value}, key('return')], name)


first_command = "export CUA_TERMINAL_TEST=retained; cd /tmp/lazyboy/cua-terminal-test; printf '中文 hello-cua\\n' > unicode.txt"
act([{'kind':'launch','application':'terminal','uri':'--title='+title},
     {'kind':'clipboard','text':encoded_command(first_command)}, key('return')], 'first')
assert (root/'unicode.txt').read_text() == '中文 hello-cua\n'
command("printf '%s\\n' \"$CUA_TERMINAL_TEST\" \"$PWD\" > persistence.txt", 'persist')
assert (root/'persistence.txt').read_text() == 'retained\n/tmp/lazyboy/cua-terminal-test\n'
command("sleep 30", 'running')
act([{'kind':'focus','title':title},key('ctrl+c')], 'interrupt', 300)
command("printf 'interrupted-ok\\n' > interrupt.txt", 'after-interrupt')
assert (root/'interrupt.txt').read_text() == 'interrupted-ok\n'
command("sleep 60 & echo $! > background.pid", 'background')
background_pid = int((root/'background.pid').read_text())
os.kill(background_pid, 0)
command("exec /usr/local/bin/lazyboy-terminal-reset", 'reset')
try:
    os.kill(background_pid, 0)
except ProcessLookupError:
    pass
else:
    raise AssertionError('reset left its background job running')
command("printf '%s\\n' \"${CUA_TERMINAL_TEST-unset}\" \"$PWD\" > /tmp/lazyboy/cua-terminal-test/reset.txt", 'after-reset')
assert (root/'reset.txt').read_text() == 'unset\n/home/lazyboy\n'
command("printf 'CUA terminal: 中文, persistence, interrupt and reset passed\\n'", 'complete')
print('Cua terminal persistence, Unicode, interrupt and reset passed', flush=True)
