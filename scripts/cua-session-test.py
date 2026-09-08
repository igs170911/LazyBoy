#!/usr/bin/env python3
"""Exercise recovery from Cua lifecycle-session expiry on a disposable desktop."""
import importlib.machinery
import importlib.util

loader = importlib.machinery.SourceFileLoader('adapter', '/usr/local/bin/lazyboy-cua-adapter-test')
spec = importlib.util.spec_from_loader(loader.name, loader)
adapter = importlib.util.module_from_spec(spec)
loader.exec_module(adapter)
adapter.wait_health()
# Mint and cache a browser binding before the session is lost.
adapter.api('/browser', {'action': 'snapshot', 'ensure': True})
adapter.smoke.cua_call('end_session', {'session': 'lazyboy-1'})
frame = adapter.observe()
assert frame['png_base64'], 'read-only recovery must return a shared-desktop frame'
page = adapter.api('/browser', {'action': 'snapshot', 'ensure': True})
assert page['ok'], 'the cached browser binding must be refreshed after session revival'
# A mutation presented directly after expiry must be rejected, not replayed.
adapter.smoke.cua_call('end_session', {'session': 'lazyboy-1'})
adapter.act({'kind': 'key', 'key': 'a'}, expect_error=True)
adapter.observe()
print('Cua session expiry: observation recovered, browser rebound, mutation not replayed', flush=True)
