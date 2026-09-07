import contextlib
import io
import json
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

class BrowserActionTest(unittest.TestCase):
    def run_action(self, request, evaluation=None):
        calls=[]
        class Socket:
            def call(self, method, params=None): calls.append((method, params)); return {}
            def close(self): pass
        output=io.StringIO()
        def evaluate(ws, expression, arg=None):
            if expression == cdp.SNAP_JS:
                return {"url":"https://example.test/next", "title":"Next", "text":"Saved", "elements":[{"id":1,"title":"Continue"}]}
            return evaluation if evaluation is not None else {"ok":True,"x":10,"y":20}
        with patch.object(cdp.sys,'argv',['cdp',json.dumps(request)]), patch.object(cdp,'probe',return_value=True), patch.object(cdp,'connect',return_value=Socket()), patch.object(cdp,'evaluate',side_effect=evaluate), patch.object(cdp,'pointer'), patch.object(cdp,'wait_for_visual_update'), patch.object(cdp,'wait_until_enabled',return_value=0), patch.object(cdp.time,'sleep'), contextlib.redirect_stdout(output):
            try: cdp.main()
            except SystemExit: pass
        return json.loads(output.getvalue()), calls

    def test_every_browser_action_returns_current_elements_and_text(self):
        for action in ['click','type','press','wait','navigate']:
            with self.subTest(action=action):
                result,_=self.run_action({"action":action,"selector":"#field","text":"hello","url":"https://example.test"})
                self.assertTrue(result['ok'])
                self.assertEqual(result['text'],'Saved')
                self.assertEqual(result['elements'][0]['title'],'Continue')

    def test_missing_field_never_types_into_previous_focus(self):
        result,calls=self.run_action({"action":"type","selector":"#gone","text":"private"},{"ok":False,"error":"element gone"})
        self.assertFalse(result['ok'])
        self.assertFalse(any(method=='Input.insertText' for method,_ in calls))

if __name__=='__main__':unittest.main()
