"""Set X11 clipboard, confirm ownership/content, then paste into the active app."""
import subprocess
import sys
import time


def run(argv, **kwargs):
    return subprocess.run(argv, check=True, timeout=2, **kwargs)


def paste(text):
    if not text:
        return
    raw = text.encode('utf-8')
    # xclip forks after reading stdin; detached descriptors avoid pipe hangs.
    run(['xclip', '-selection', 'clipboard', '-in'], input=raw,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    deadline = time.monotonic() + 2
    while True:
        actual = run(['xclip', '-selection', 'clipboard', '-out'], stdout=subprocess.PIPE,
                     stderr=subprocess.DEVNULL).stdout
        if actual == raw:
            break
        if time.monotonic() >= deadline:
            raise RuntimeError('clipboard synchronization timed out; nothing pasted')
        time.sleep(.02)
    key_for_active_app('v')


def key_for_active_app(key):
    window = run(['xdotool', 'getactivewindow'], stdout=subprocess.PIPE).stdout.decode().strip()
    wmclass = run(['xprop', '-id', window, 'WM_CLASS'], stdout=subprocess.PIPE).stdout.decode().lower()
    terminal = any(name in wmclass for name in ('terminal', 'xterm', 'kitty', 'alacritty', 'konsole'))
    run(['xdotool', 'key', '--clearmodifiers', ('ctrl+shift+' if terminal else 'ctrl+') + key],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def copy_selection():
    key_for_active_app('c')
    # Wait for the application to process the shortcut before reading selection.
    time.sleep(.1)
    return run(['xclip', '-selection', 'clipboard', '-out'], stdout=subprocess.PIPE,
               stderr=subprocess.DEVNULL).stdout.decode('utf-8')



if __name__ == '__main__':
    if len(sys.argv)>1 and sys.argv[1]=='copy':
        sys.stdout.write(copy_selection())
    else:
        paste(sys.stdin.read())
