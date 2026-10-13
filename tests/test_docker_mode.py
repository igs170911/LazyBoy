"""LazyBoy running in Docker: bot computers mount from the product's volume and are reached on its network."""
import os
from pathlib import Path
import pytest
from server import computers


@pytest.fixture
def in_docker(monkeypatch):
    monkeypatch.setattr(computers, "DATA_VOLUME", "lazyboy-data")
    monkeypatch.setattr(computers, "NETWORK", "lazyboy")
    monkeypatch.setattr(computers, "DATA", Path("/data"))


def test_host_install_mounts_host_folders():
    assert computers.mount(Path("/srv/lazyboy/.data/ipc"), "/product-ipc", readonly=True) == ["-v", "/srv/lazyboy/.data/ipc:/product-ipc:ro"]


def test_docker_install_mounts_the_same_folder_from_the_volume(in_docker):
    assert computers.mount(Path("/data/bots/abc/hermes"), "/hermes") == \
        ["--mount", "type=volume,src=lazyboy-data,dst=/hermes,volume-subpath=bots/abc/hermes"]
    assert computers.mount(Path("/data/ipc"), "/product-ipc", readonly=True)[1].endswith("volume-subpath=ipc,readonly")


def test_docker_install_keeps_its_own_user_even_under_rootless_docker(in_docker, monkeypatch):
    monkeypatch.setattr(computers, "_remapped", True)
    assert computers.container_user() == (os.getuid(), os.getgid())
    assert computers.user_flag() == f"{os.getuid()}:{os.getgid()}"
    assert computers.reclaim(Path("/data/bots/abc")) is False


def test_rootless_host_install_runs_commands_as_container_root(monkeypatch):
    monkeypatch.setattr(computers, "_remapped", True)
    assert computers.user_flag() == "0:0"


def running(networks=None, ports=None):
    return {"Name": "/lazyhermes-bot-abc", "NetworkSettings": {"Networks": networks or {}, "Ports": ports or {}}}


def test_host_install_reaches_computers_on_published_ports():
    box = computers.Computers.__new__(computers.Computers)
    item = running(ports={"8642/tcp": [{"HostPort": "40001"}], "6080/tcp": [{"HostPort": "40002"}]})
    assert box.address(item) == ("127.0.0.1", 40001, 40002)
    assert box.address(running()) is None


def test_docker_install_reaches_computers_by_name(in_docker):
    box = computers.Computers.__new__(computers.Computers)
    assert box.address(running(networks={"lazyboy": {}})) == ("lazyhermes-bot-abc", 8642, 6080)
    # A computer made by a host install is not on the network: never mistaken for reachable.
    assert box.address(running(networks={"bridge": {}}, ports={"8642/tcp": [{"HostPort": "1"}], "6080/tcp": [{"HostPort": "2"}]})) is None
