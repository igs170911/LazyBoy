#!/usr/bin/env python3
"""Clipboard roundtrip and terminal paste through Cua in a disposable desktop."""
import base64
import importlib.machinery
import importlib.util
import time
from pathlib import Path
loader = importlib.machinery.SourceFileLoader('adapter','/usr/local/bin/lazyboy-cua-adapter-test')
spec = importlib.util.spec_from_loader(loader.name,loader)
adapter = importlib.util.module_from_spec(spec)
loader.exec_module(adapter)
adapter.wait_health()
name='LazyBoy clipboard test ' + str(time.monotonic_ns())
out=Path('/tmp/lazyboy/cua-clipboard-test.txt')
out.unlink(missing_ok=True)

def act(actions):
    return adapter.api('/act',{'actions':actions,'observe':True,'settle_ms':500})
def key(value):
    return {'kind':'key','key':value}

act([{'kind':'launch','application':'terminal','uri':'--title='+name}])
act([{'kind':'focus','title':name},{'kind':'clipboard','text':"printf '中文🙂\\nsecond line\\n' > /tmp/lazyboy/cua-clipboard-test.txt"},key('return')])
assert out.read_text()=='中文🙂\nsecond line\n',repr(out.read_bytes())
# Multiline paste must be bracketed into the same terminal, not silently lose
# characters or submit incomplete lines through an X11 typing fallback.
out.unlink()
act([{'kind':'focus','title':name},{'kind':'clipboard','text':"printf '%s' '中文\n第二行🙂' > /tmp/lazyboy/cua-clipboard-test.txt"},key('return')])
# The configured zsh confirms bracketed multiline paste before execution.
act([key('return')])
assert out.read_text()=='中文\n第二行🙂',repr(out.read_bytes())
act([{'kind':'focus','title':name},key('ctrl+shift+a')])
result=act([{'kind':'copyselection'}])
assert '第二行' in result['clipboardText'],result
print('Cua clipboard Unicode, multiline terminal paste and copy roundtrip passed',flush=True)
