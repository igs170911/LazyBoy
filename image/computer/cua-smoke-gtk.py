#!/usr/bin/env python3
"""Tiny GTK window used by the Cua smoke test to verify native click/type."""

from pathlib import Path
import json
import os

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, Gdk, GLib

ROOT = Path(os.environ.get("LAZYBOY_SMOKE_ROOT", "/tmp/lazyboy"))
CLICKED = ROOT / "cua-smoke-clicked"
TYPED = ROOT / "cua-smoke-typed"


class SmokeWindow(Gtk.Window):
    def __init__(self) -> None:
        super().__init__(title=os.environ.get("LAZYBOY_SMOKE_TITLE", "LazyBoy Cua Smoke"))
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

        self.drag_events = []
        self.drag = Gtk.DrawingArea()
        self.drag.set_size_request(400, 80)
        self.drag.add_events(Gdk.EventMask.BUTTON_PRESS_MASK | Gdk.EventMask.BUTTON_RELEASE_MASK | Gdk.EventMask.POINTER_MOTION_MASK | Gdk.EventMask.SCROLL_MASK)
        for signal in ("button-press-event", "button-release-event", "motion-notify-event", "scroll-event"):
            self.drag.connect(signal, self.on_drag_event, signal)
        box.pack_start(self.drag, False, False, 0)
        GLib.idle_add(self.save_drag_geometry)

        click.get_accessible().set_name("Smoke Click")
        self.entry.get_accessible().set_name("Smoke Entry")
        save.get_accessible().set_name("Smoke Save")
        self.status.get_accessible().set_name("Smoke Status")
        self.connect("destroy", Gtk.main_quit)

    def save_drag_geometry(self):
        # Root coords of the drawing area, not widget-local origin.
        top = self.get_window().get_origin()
        alloc = self.drag.get_allocation()
        ROOT.mkdir(parents=True, exist_ok=True)
        (ROOT / "cua-smoke-drag-geometry.json").write_text(json.dumps({
            "x": int(top[-2]) + int(alloc.x),
            "y": int(top[-1]) + int(alloc.y),
        }))
        return False

    def on_drag_event(self, _widget, event, signal):
        self.drag_events.append(signal)
        (ROOT / "cua-smoke-drag.json").write_text(json.dumps(self.drag_events))
        return False

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
