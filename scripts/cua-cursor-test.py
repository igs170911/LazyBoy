#!/usr/bin/env python3
"""Check named cursor sessions and save a real desktop frame for visual review.

Run as the desktop user in a disposable computer container. The screenshot is
intentional evidence: Cua's Linux cursor-state response does not report geometry,
and a successful state call alone cannot prove that the overlay was drawn.
"""
import base64
import importlib.machinery
import importlib.util
import subprocess
from pathlib import Path

loader = importlib.machinery.SourceFileLoader('adapter', '/usr/local/bin/lazyboy-cua-adapter-test')
spec = importlib.util.spec_from_loader(loader.name, loader)
adapter = importlib.util.module_from_spec(spec)
loader.exec_module(adapter)

adapter.wait_health()
adapter.api('/browser', {'action': 'snapshot', 'ensure': True})
name_file = Path('/tmp/lazyboy/screen-1.agent-name')
previous = name_file.read_bytes() if name_file.exists() else None
names = ['Alice', '小幫手 Alice']
try:
    for index, name in enumerate(names):
        subprocess.run(['lazyboy-screen', 'ensure', '0', '', name], check=True)
        assert name_file.read_text() == name
        # This also exercises rebinding after changing a screen's public label.
        adapter.api('/browser', {'action': 'snapshot', 'ensure': True})
        adapter.api('/browser', {'action': 'navigate', 'url': 'about:blank', 'ensure': True})
        frame = adapter.api('/act', {
            'actions': [{'kind': 'pointer', 'type': 'click', 'x': 640, 'y': 400}],
            'observe': True, 'settle_ms': 250,
        })
        state = adapter.smoke.cua_call('get_agent_cursor_state', {'session': name})['parsed']
        assert state['session'] == name and state['enabled'], state
        # Use /observe rather than depending on the action response envelope.
        frame = adapter.observe()
        Path(f'/tmp/cua-cursor-{index}.png').write_bytes(base64.b64decode(frame['png_base64']))
        adapter.smoke.cua_call('end_session', {'session': name})
        adapter.observe()  # Read recovery must revive this name, not lazyboy-1.
        state = adapter.smoke.cua_call('get_agent_cursor_state', {'session': name})['parsed']
        assert state['session'] == name
        adapter.smoke.cua_call('end_session', {'session': name})
finally:
    if previous is None:
        name_file.unlink(missing_ok=True)
    else:
        name_file.write_bytes(previous)
print('Named Cua cursor: English/Chinese labels, rename/rebind, expiry recovery passed')
print('Visually inspect /tmp/cua-cursor-0.png and /tmp/cua-cursor-1.png for rendered names')
