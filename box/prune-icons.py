#!/usr/bin/python3
"""Shrink the Papirus icon themes to what this computer shows (build time only, box/Dockerfile).

Papirus is SVG, so GTK scales the nearest size losslessly: only the sizes named on the command line stay, plus
the symbolic icons. Its apps/ folders draw ~3,800 programs; only icons that an installed launcher, an XFCE panel
plugin or LazyBoy's own launchers name stay (with the files their links point to). A program installed later
brings its own hicolor icon, and a missing one falls back to the generic icon.

Usage: prune-icons.py SIZE... (e.g. 16x16 22x22 32x32 48x48)
"""
import glob
import os
import shutil
import sys

THEMES = ["/usr/share/icons/Papirus", "/usr/share/icons/Papirus-Dark"]
# Named by LazyBoy's launchers (box/xfce/*.desktop), which are copied in after this runs.
EXTRA = {"web-browser", "utilities-terminal", "system-file-manager"}
# Desktop and settings icons XFCE and GTK look up by name rather than through a launcher.
PREFIXES = ("org.xfce.", "xfce", "applications-", "preferences-", "system-", "utilities-", "accessories-", "help-",
            "libreoffice", "chromium", "thunar", "mousepad", "ristretto", "xarchiver", "application-", "user-")

sizes = set(sys.argv[1:]) | {"symbolic"}
names = set(EXTRA)
for pattern in ("/usr/share/applications/*.desktop", "/usr/share/xfce4/panel/plugins/*.desktop", "/etc/xdg/autostart/*.desktop"):
    for path in glob.glob(pattern):
        with open(path, encoding="utf-8", errors="replace") as handle:
            names.update(line.split("=", 1)[1].strip() for line in handle if line.startswith("Icon=") and "/" not in line)

for theme in THEMES:
    for entry in os.scandir(theme):
        if entry.name in sizes: continue
        # HiDPI folders like 16x16@2x are links to another size.
        if entry.is_symlink(): os.remove(entry.path)
        elif entry.is_dir(): shutil.rmtree(entry.path)

def hops(path):
    """The path and every link it passes through, as real paths."""
    path = os.path.join(os.path.realpath(os.path.dirname(path)), os.path.basename(path))
    while True:
        yield path
        if not os.path.islink(path): return
        path = os.path.normpath(os.path.join(os.path.dirname(path), os.readlink(path)))
        path = os.path.join(os.path.realpath(os.path.dirname(path)), os.path.basename(path))


# Everything outside apps/ stays, and so does whatever those icons (mimetypes, places...) link to.
keep, apps = set(), set()
for theme in THEMES:
    for folder, dirs, files in os.walk(theme, followlinks=True):
        for name in files + [d for d in dirs if os.path.islink(os.path.join(folder, d))]:
            path = os.path.join(folder, name)
            real = os.path.join(os.path.realpath(folder), name)
            if os.path.basename(os.path.realpath(folder)) == "apps" and not (name.rsplit(".", 1)[0] in names or name.startswith(PREFIXES)):
                apps.add(real)
            else:
                keep.update(hops(path))
removed = apps - keep
for path in removed:
    os.remove(path)
print(f"prune-icons: removed {len(removed)} app icons, kept sizes {sorted(sizes)}")
