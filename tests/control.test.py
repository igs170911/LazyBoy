import importlib.util
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch


def module(name, path):
    spec=importlib.util.spec_from_file_location(name,path)
    mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod);return mod

clipboard=module('clipboard',Path('crates/control/src/clipboard.py'))
cdp=module('cdp',Path('crates/control/src/cdp.py'))

class ClipboardTest(unittest.TestCase):
    def test_confirmed_unicode_then_terminal_shortcut(self):
        calls=[]
        def fake(argv,**kwargs):
            calls.append(argv)
            data=b''
            if '-out' in argv: data='中文\nemoji🙂'.encode()
            if 'getactivewindow' in argv:data=b'123'
            if 'WM_CLASS' in argv:data=b'xfce4-terminal'
            return subprocess.CompletedProcess(argv,0,stdout=data)
        with patch.object(clipboard,'run',fake): clipboard.paste('中文\nemoji🙂')
        self.assertEqual(calls[-1][-1],'ctrl+shift+v')
        self.assertEqual(sum('key' in a for a in calls),1)
    def test_no_paste_when_sync_fails(self):
        calls=[]
        def fake(argv,**kwargs):calls.append(argv);return subprocess.CompletedProcess(argv,0,stdout=b'stale')
        with patch.object(clipboard,'run',fake),patch.object(clipboard.time,'monotonic',side_effect=[0,3]):
            with self.assertRaises(RuntimeError):clipboard.paste('new')
        self.assertFalse(any('key' in a for a in calls))
    def test_cdp_handles_events_before_response(self):
        class Socket:
            def __init__(self):self.items=iter(['{"method":"Page.event"}','{"id":1,"result":{"ok":true}}'])
            def send(self,x):pass
            def settimeout(self,x):pass
            def recv(self):return next(self.items)
        ws=cdp.Ws.__new__(cdp.Ws);ws.sock=Socket();ws.n=0
        self.assertEqual(ws.call('Runtime.test'),{'ok':True})

if __name__=='__main__':unittest.main()
