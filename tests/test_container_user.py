"""Bot computers under rootless Docker or userns-remap keep their files writable by the product."""
import os
import subprocess
from pathlib import Path
import pytest
from server import computers


@pytest.fixture(autouse=True)
def fresh_detection(monkeypatch):
    monkeypatch.setattr(computers, "_remapped", None)


def fake_info(monkeypatch, output):
    calls = []
    def run(args, **kwargs):
        calls.append(args)
        return subprocess.CompletedProcess(args, 0, stdout=output, stderr="")
    monkeypatch.setattr(computers.subprocess, "run", run)
    return calls


def test_plain_docker_runs_bots_as_the_host_user(monkeypatch):
    fake_info(monkeypatch, '["name=seccomp,profile=builtin","name=cgroupns"]')
    assert computers.container_user() == (os.getuid(), os.getgid())
    assert computers.reclaim(Path("/tmp/x")) is False


@pytest.mark.parametrize("option", ["name=rootless", "name=userns"])
def test_remapped_docker_keeps_container_root(monkeypatch, option):
    calls = fake_info(monkeypatch, f'["name=seccomp,profile=builtin","{option}"]')
    assert computers.container_user() == (0, 0)
    computers.container_user()
    assert len(calls) == 1  # detected once


def test_missing_docker_falls_back_to_the_host_user(monkeypatch):
    def run(args, **kwargs): raise FileNotFoundError("docker")
    monkeypatch.setattr(computers.subprocess, "run", run)
    assert computers.container_user() == (os.getuid(), os.getgid())


def test_reclaim_chowns_the_bot_folders_to_container_root(monkeypatch, tmp_path):
    calls = fake_info(monkeypatch, '["name=rootless"]')
    assert computers.reclaim(tmp_path) is True
    chown = calls[-1]
    assert chown[:2] == ["docker", "run"] and "chown" in chown and chown[-3:] == ["-R", "0:0", "/reclaim"]
    for name in ("hermes", "home", "workspace"): assert f"{tmp_path / name}:/reclaim/{name}" in chown


def test_prepare_takes_files_back_once_after_permission_denied(monkeypatch, tmp_path):
    box = computers.Computers.__new__(computers.Computers)
    box.store = type("S", (), {"agent_directory": lambda self, id: tmp_path})()
    attempts = []
    def prepare(id):
        attempts.append(id)
        if len(attempts) == 1: raise PermissionError(13, "Permission denied", "config.yaml")
        return tmp_path
    box._prepare = prepare
    reclaimed = []
    monkeypatch.setattr(computers, "reclaim", lambda root: reclaimed.append(root) or True)
    assert box.prepare("bot") == tmp_path
    assert attempts == ["bot", "bot"] and reclaimed == [tmp_path]


def test_prepare_still_fails_when_nothing_can_be_reclaimed(monkeypatch, tmp_path):
    box = computers.Computers.__new__(computers.Computers)
    box.store = type("S", (), {"agent_directory": lambda self, id: tmp_path})()
    def prepare(id): raise PermissionError(13, "Permission denied", "config.yaml")
    box._prepare = prepare
    monkeypatch.setattr(computers, "reclaim", lambda root: False)
    with pytest.raises(PermissionError): box.prepare("bot")
