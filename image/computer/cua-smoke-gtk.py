#!/usr/bin/env python3
"""Tiny GTK window used by the Cua smoke test to verify native click/type."""

from pathlib import Path

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import Gtk

ROOT = Path("/tmp/lazyboy")
CLICKED = ROOT / "cua-smoke-clicked"
TYPED = ROOT / "cua-smoke-typed"


class SmokeWindow(Gtk.Window):
    def __init__(self) -> None:
        super().__init__(title="LazyBoy Cua Smoke")
        self.set_default_size(480, 240)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        box.set_margin_top(16)
        box.set_margin_bottom(16)
        box.set_margin_start(16)
        box.set_margin_end(16)
        self.add(box)

        self.status = Gtk.Label(label="ready")
        self.entry = Gtk.Entry()
        self.entry.set_placeholder_text("type here")
        click = Gtk.Button(label="Smoke Click")
        save = Gtk.Button(label="Smoke Save")
        click.connect("clicked", self.on_click)
        save.connect("clicked", self.on_save)
        box.pack_start(self.status, False, False, 0)
        box.pack_start(click, False, False, 0)
        box.pack_start(self.entry, False, False, 0)
        box.pack_start(save, False, False, 0)

        click.get_accessible().set_name("Smoke Click")
        self.entry.get_accessible().set_name("Smoke Entry")
        save.get_accessible().set_name("Smoke Save")
        self.status.get_accessible().set_name("Smoke Status")
        self.connect("destroy", Gtk.main_quit)

    def on_click(self, _button: Gtk.Button) -> None:
        ROOT.mkdir(parents=True, exist_ok=True)
        CLICKED.write_text("clicked\n", encoding="utf-8")
        self.status.set_text("clicked-ok")

    def on_save(self, _button: Gtk.Button) -> None:
        ROOT.mkdir(parents=True, exist_ok=True)
        TYPED.write_text(self.entry.get_text(), encoding="utf-8")
        self.status.set_text("typed-ok")


def main() -> None:
    ROOT.mkdir(parents=True, exist_ok=True)
    window = SmokeWindow()
    window.show_all()
    Gtk.main()


if __name__ == "__main__":
    main()
