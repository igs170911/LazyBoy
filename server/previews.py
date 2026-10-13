"""Previews the browser cannot draw by itself: Office documents and archives.

Office files are converted by LibreOffice on the bot's own computer: documents and slides become a PDF, spreadsheets
an HTML page with every sheet. Results are kept next to the bot's other caches, keyed by the file's size and change
time, so opening the same file again is instant and an edited file is converted again. Archives are listed here.
"""
import asyncio
import hashlib
import os
import tarfile
import zipfile
from pathlib import Path

from . import computers as computers_module

TO_PDF = {".doc", ".docx", ".docm", ".dot", ".dotx", ".odt", ".ott", ".rtf", ".wpd", ".pages",
          ".ppt", ".pptx", ".pptm", ".pps", ".ppsx", ".odp", ".otp", ".key", ".vsd", ".vsdx", ".odg", ".pub"}
TO_HTML = {".xls", ".xlsx", ".xlsm", ".xlsb", ".ods", ".ots", ".numbers"}
CACHE = ".cache/lazyboy-preview"
CACHE_DAYS = 7
ARCHIVE_ENTRIES = 2000
CONVERT_SECONDS = 120


def office_kind(path: Path):
    suffix = path.suffix.lower()
    return "pdf" if suffix in TO_PDF else "html" if suffix in TO_HTML else None


def container_path(root: Path, target: Path):
    """Where `target` (under the bot's workspace or home on this machine) is inside its computer."""
    for host, inside in (("workspace", "/workspace"), ("home", "/home/box")):
        base = (root / host).resolve()
        if target.is_relative_to(base): return f"{inside}/{target.relative_to(base).as_posix()}"
    raise ValueError("檔案必須位於這個 Bot 的工作目錄")


class Previews:
    def __init__(self, store, computers):
        self.store, self.computers = store, computers
        self.locks = {}

    def cached(self, id, target: Path, kind):
        stat = target.stat()
        key = hashlib.sha1(f"{target}:{stat.st_size}:{stat.st_mtime_ns}".encode()).hexdigest()[:20]
        folder = self.store.agent_directory(id) / "home" / CACHE / key
        return folder, folder / f"{target.stem}.{kind}"

    async def office(self, id, target: Path):
        """The converted file for an Office document, converting it on the bot's computer when needed."""
        kind = office_kind(target)
        if not kind: raise ValueError("這種檔案沒有 Office 預覽")
        folder, out = self.cached(id, target, kind)
        if out.exists(): return out, kind
        async with self.locks.setdefault(id, asyncio.Lock()):
            if out.exists(): return out, kind
            status = await self.computers.status(id, refresh=True)
            if not status.get("ready"): await self.computers.start(id)
            folder.mkdir(parents=True, exist_ok=True)
            source = container_path(self.store.agent_directory(id), target)
            inside = f"/home/box/{CACHE}/{folder.name}"
            # A profile of its own, so a LibreOffice window open on the desktop does not take over the conversion.
            code, _, err = await computers_module.docker(
                "exec", "-u", f"{os.getuid()}:{os.getgid()}", "-e", "HOME=/tmp/lazyboy-preview", self.computers.name(id),
                "timeout", str(CONVERT_SECONDS), "soffice", "-env:UserInstallation=file:///tmp/lazyboy-preview/profile",
                "--headless", "--norestore", "--nologo", "--convert-to", kind, "--outdir", inside, source,
                check=False, timeout=CONVERT_SECONDS + 15)
            if code in (126, 127) or "executable file not found" in err:
                raise ValueError("這台電腦還沒有 Office 檢視工具，請在電腦區按「更新電腦環境」")
            if not out.exists():
                raise ValueError("無法轉換這個檔案，請下載後在電腦上開啟" + (f"（{err.strip()[-200:]}）" if err.strip() else ""))
        return out, kind


def archive_entries(target: Path):
    """What is inside a zip or tar archive: names, sizes and folders, without extracting anything."""
    rows, total = [], 0
    try:
        if zipfile.is_zipfile(target):
            with zipfile.ZipFile(target) as archive:
                infos = archive.infolist(); total = len(infos)
                for info in infos[:ARCHIVE_ENTRIES]:
                    rows.append({"name": info.filename, "bytes": info.file_size, "dir": info.is_dir()})
        elif tarfile.is_tarfile(target):
            with tarfile.open(target) as archive:
                for member in archive:
                    total += 1
                    if len(rows) < ARCHIVE_ENTRIES: rows.append({"name": member.name, "bytes": member.size, "dir": member.isdir()})
        else:
            raise ValueError("無法讀取這種壓縮檔，請下載後開啟")
    except (zipfile.BadZipFile, tarfile.TarError, EOFError, OSError) as exc:
        raise ValueError("壓縮檔已損毀或無法讀取") from exc
    return {"entries": rows, "total": total, "truncated": total > len(rows)}
