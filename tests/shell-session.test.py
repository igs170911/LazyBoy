"""The agent's persistent terminal (image/computer/lazyboy-shell).

Run on the host or inside the desktop image: it needs tmux only, and talks to a
private tmux socket in a temp directory so it never touches a real session.
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'image' / 'computer' / 'lazyboy-shell'


@unittest.skipUnless(shutil.which('tmux'), 'tmux is not installed')
class ShellSessionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.env = {
            'PATH': os.environ.get('PATH', '/usr/bin:/bin'),
            'HOME': cls.temp.name,
            'TMUX_TMPDIR': f'{cls.temp.name}/tmux',
            'LAZYBOY_SHELL_STATE': f'{cls.temp.name}/state',
            'LANG': 'C.UTF-8',
            'LC_ALL': 'C.UTF-8',
        }
        os.makedirs(cls.env['TMUX_TMPDIR'], exist_ok=True)
        os.makedirs(cls.env['LAZYBOY_SHELL_STATE'], exist_ok=True)

    @classmethod
    def tearDownClass(cls):
        subprocess.run(['tmux', 'kill-server'], env=cls.env,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        cls.temp.cleanup()

    def shell(self, *argv, timeout=40):
        return subprocess.run([str(SCRIPT), *argv], env=self.env,
                              capture_output=True, text=True, timeout=timeout)

    def run_(self, command, session='main', wait_ms=8000):
        return self.shell('run', session, str(wait_ms), command)

    def test_reports_output_and_exit_code(self):
        result = self.run_('echo ready')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('status=done session=main exit=0', result.stdout)
        self.assertIn('ready', result.stdout)
        failed = self.run_('echo boom >&2; exit 7')
        self.assertIn('exit=7', failed.stdout)
        self.assertIn('boom', failed.stdout)

    def test_working_directory_and_env_survive_between_calls(self):
        # The whole point: this is one terminal, not a fresh process per call.
        self.run_('cd /tmp && export LAZYBOY_MARKER=kept')
        here = self.run_('pwd; echo $LAZYBOY_MARKER')
        self.assertIn('/tmp', here.stdout)
        self.assertIn('kept', here.stdout)

    def test_a_still_running_command_is_reported_not_typed_over(self):
        slow = self.run_('echo starting; sleep 30', wait_ms=500)
        self.assertIn('status=running', slow.stdout)
        self.assertIn('starting', slow.stdout)
        crowded = self.run_('echo second', wait_ms=1000)
        self.assertIn('status=running', crowded.stdout)
        self.assertIn('nothing was typed', crowded.stdout)
        self.assertNotIn('second', crowded.stdout)
        # Ctrl-C is the human answer: it releases the terminal, and the next
        # command runs in the same shell.
        self.assertIn('status=sent', self.shell('keys', 'main', 'C-c').stdout)
        after = self.run_('echo usable', wait_ms=8000)
        self.assertIn('status=done', after.stdout)
        self.assertIn('usable', after.stdout)

    def test_log_reads_the_terminal_without_typing_anything(self):
        self.run_('echo logged', session='poll')
        logged = self.shell('log', 'poll', '40')
        self.assertIn('status=idle', logged.stdout)
        self.assertIn('logged', logged.stdout)

    def test_a_command_that_exits_the_shell_leaves_a_usable_terminal(self):
        # `exit` is a command like any other: the code is reported, and the next
        # call gets the same terminal back where it stood.
        exited = self.run_('export LB_KEEP=through_exit; cd /etc; exit 3',
                           session='exiter')
        self.assertIn('exit=3', exited.stdout)
        self.assertIn('restarted', exited.stdout)
        after = self.run_('pwd; echo marker=$LB_KEEP', session='exiter')
        self.assertIn('status=done', after.stdout)
        self.assertIn('/etc', after.stdout)
        self.assertIn('marker=through_exit', after.stdout)

    def test_a_shell_replaced_by_the_command_is_adopted(self):
        # `exec bash` swallows the end marker; the terminal must not stay locked.
        self.run_('exec bash', session='swapped', wait_ms=1500)
        after = self.run_('echo recovered', session='swapped')
        self.assertIn('status=done', after.stdout)
        self.assertIn('recovered', after.stdout)

    def test_log_reports_whether_the_command_is_still_running(self):
        self.run_('echo starting; sleep 30', session='watched', wait_ms=500)
        busy = self.shell('log', 'watched', '20')
        self.assertIn('status=running', busy.stdout)
        self.assertIn('starting', busy.stdout)
        self.shell('keys', 'watched', 'C-c')
        idle = self.shell('log', 'watched', '20')
        self.assertIn('status=idle', idle.stdout)

    def test_sessions_are_listed_and_resettable(self):
        self.run_('echo hi', session='alpha')
        self.assertIn('session=alpha state=open', self.shell('list').stdout)
        self.assertIn('session=alpha', self.shell('reset', 'alpha').stdout)
        self.run_('pwd', session='alpha')

    def test_rejects_session_names_that_are_not_boring(self):
        for bad in ('', 'two words', 'a;rm -rf /'):
            result = self.run_('echo nope', session=bad)
            self.assertEqual(result.returncode, 2, bad)
            self.assertIn('session name', result.stderr)

    def test_unknown_subcommand_is_an_error(self):
        result = self.shell('nonsense')
        self.assertEqual(result.returncode, 2)
        self.assertIn('usage:', result.stderr)


if __name__ == '__main__':
    unittest.main()
