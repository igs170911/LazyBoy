import importlib.util
from pathlib import Path

import pytest

spec = importlib.util.spec_from_file_location("switch_services", Path(__file__).resolve().parents[1] / "scripts/switch-to-hermes.py")
switch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(switch)


def test_container_selection_keeps_other_compose_projects_and_data_dirs():
    def compose(path): return {"Config":{"Labels":{"com.docker.compose.project.working_dir":str(path)}}}
    assert switch.container_owner(compose(switch.OLD / "routing")) == "old"
    assert switch.container_owner(compose(switch.ROOT / "routing")) == "new-classifier"
    assert switch.container_owner(compose(switch.OLD.parent / "another/routing")) is None
    assert switch.container_owner(compose(Path(str(switch.OLD) + "-other/routing"))) is None
    bot={"Name":"/lazyhermes-bot-abc","Config":{"Labels":{"lazyhermes.managed":"true","lazyhermes.bot":"abc"}},"Mounts":[{"Destination":"/hermes","Source":str(switch.DATA / "bots/abc/hermes")}]}
    assert switch.container_owner(bot) == "new-bot"
    # Bots live under bots/ (Store.agent_directory); an agents/ folder is nobody's.
    bot["Mounts"][0]["Source"]=str(switch.DATA / "agents/abc/hermes")
    assert switch.container_owner(bot) is None
    bot["Mounts"][0]["Source"]="/tmp/unrelated/hermes"
    assert switch.container_owner(bot) is None
    assert switch.container_owner({"Name":"/lazyboy-box-abc","Config":{"Image":"other-box:local"}}) is None
    assert switch.container_owner({"Name":"/lazyboy-box-abc","Config":{"Image":"lazyboy-box:local"}}) == "old"


def test_process_selection_does_not_kill_codex_tests_or_other_projects():
    root=switch.ROOT
    assert switch.runtime_owner(root, "/usr/bin/python3", ["python3","-m","server.serve"]) == "new"
    assert switch.runtime_owner(root, "/usr/bin/python3", ["python3","-m","pytest"]) is None
    assert switch.runtime_owner(root, "/usr/bin/codex", ["codex","stop server.serve"]) is None
    assert switch.runtime_owner(root.parent / "other", "/usr/bin/python3", ["python3","-m","server.serve"]) is None
    assert switch.runtime_owner(switch.OLD, "/usr/bin/node", ["npm run dev --host 0.0.0.0"]) == "old"
    assert switch.runtime_owner(switch.OLD / "web", "/usr/bin/node", ["node",str(switch.OLD / "web/node_modules/.bin/vite")]) == "old"
    assert switch.runtime_owner(switch.OLD, switch.OLD / "target/debug/lazyboy", ["lazyboy","serve"]) == "old"


def test_docker_denial_prevents_all_service_mutations(monkeypatch):
    calls=[]
    def denied(args, **kwargs):
        calls.append(args)
        raise RuntimeError("Docker permission denied")
    monkeypatch.setattr(switch,"run",denied)
    monkeypatch.setattr(switch.sys,"argv",["switch-to-hermes.py"])
    with pytest.raises(RuntimeError,match="permission denied"): switch.main()
    assert calls == [["docker","info","--format","{{.ServerVersion}}"]]


def test_reused_pid_is_not_signalled(monkeypatch):
    calls=[]
    old={987654:{"pid":987654,"parent":1,"stamp":"old","owner":"old","state":"S"}}
    monkeypatch.setattr(switch,"processes",lambda:{987654:{"stamp":"different","owner":None,"state":"S"}})
    monkeypatch.setattr(switch.os,"kill",lambda *args:calls.append(args))
    switch.stop_processes(old)
    assert calls == []


def test_container_recreation_requires_all_durable_state_on_host():
    item={"Config":{"Labels":{"lazyhermes.bot":"abc"}},"Mounts":[{"Type":"bind","Destination":target,"Source":str(switch.DATA / "bots" / "abc" / folder)} for target,folder in (("/hermes","hermes"),("/home/box","home"),("/workspace","workspace"))]}
    assert switch.persistent_computer(item)
    item["Mounts"].pop()
    assert not switch.persistent_computer(item)
    item["Mounts"].append({"Type":"volume","Destination":"/workspace","Source":"/var/lib/docker/volumes/shared/_data"})
    assert not switch.persistent_computer(item)


def test_process_scan_detects_old_daemon_after_its_binary_was_rebuilt(tmp_path):
    process=tmp_path / "924534"
    process.mkdir()
    fields=["S","1",*["0"]*17,"777"]
    (process / "stat").write_text("924534 (lazyboy) " + " ".join(fields))
    (process / "cmdline").write_bytes(str(switch.OLD / "target/debug/lazyboy").encode() + b"\0serve\0")
    (process / "cwd").symlink_to(switch.OLD, target_is_directory=True)
    (process / "exe").symlink_to(str(switch.OLD / "target/debug/lazyboy") + " (deleted)")
    assert switch.processes(tmp_path)[924534]["owner"] == "old"


def test_start_forces_public_bind_and_verifies_the_actual_listener(monkeypatch):
    calls=[]
    monkeypatch.setenv("LAZYHERMES_BIND","127.0.0.1")
    monkeypatch.setattr(switch,"run",lambda args,**kwargs:calls.append((args,kwargs)))
    monkeypatch.setattr(switch,"verify_public_listener",lambda:calls.append("verified"))
    switch.start_product()
    assert calls[0][1]["env"]["LAZYHERMES_BIND"] == "0.0.0.0"
    assert calls[1] == "verified"


def test_public_bind_check_rejects_loopback_and_another_process_socket(tmp_path,monkeypatch):
    data=tmp_path / "data";data.mkdir()
    (data / "service.lock").write_text("1032694")
    proc=tmp_path / "proc";fds=proc / "1032694/fd";fds.mkdir(parents=True)
    (fds / "8").symlink_to("socket:[999]")
    monkeypatch.setattr(switch,"DATA",data)
    row={"address":"127.0.0.1","port":switch.PORT,"inode":"999"}
    monkeypatch.setattr(switch,"listeners",lambda proc:[row])
    with pytest.raises(RuntimeError,match="0.0.0.0"): switch.verify_public_listener(proc)
    row["address"]="0.0.0.0";row["inode"]="different-process"
    with pytest.raises(RuntimeError,match="0.0.0.0"): switch.verify_public_listener(proc)
    row["inode"]="999"
    switch.verify_public_listener(proc)


def test_remaining_old_listener_cannot_be_reported_as_stopped(monkeypatch):
    monkeypatch.setattr(switch,"processes",lambda:{})
    monkeypatch.setattr(switch,"listeners",lambda:[{"address":"0.0.0.0","port":8787,"inode":"999"}])
    with pytest.raises(RuntimeError,match="8787"): switch.verify_old_stopped()


def test_stop_old_keeps_new_api_and_computers_running(monkeypatch):
    rows={1:{"owner":"old"},2:{"owner":"new"}}
    monkeypatch.setattr(switch,"processes",lambda:rows)
    monkeypatch.setattr(switch,"verify_process_visibility",lambda rows:None)
    stopped=[]
    monkeypatch.setattr(switch,"stop_processes",lambda selected:stopped.extend(selected))
    old={"Id":"old-container","Name":"/lazyboy-box-abc","Config":{"Image":"lazyboy-box:local"},"State":{"Running":True}}
    new={"Id":"new-container","Name":"/classifier","Config":{"Labels":{"com.docker.compose.project.working_dir":str(switch.ROOT / "routing")}},"State":{"Running":True}}
    inventory=iter(([old,new],[new]))
    monkeypatch.setattr(switch,"containers",lambda:next(inventory))
    calls=[]
    monkeypatch.setattr(switch,"run",lambda args,**kwargs:calls.append(args))
    monkeypatch.setattr(switch,"verify_old_stopped",lambda:None)
    switch.stop_old_services()
    assert stopped == [1]
    assert calls == [["docker","stop","-t","20","old-container"]]


def test_kernel_listener_table_reports_actual_ipv4_and_ipv6_bindings(tmp_path):
    net=tmp_path / "net";net.mkdir()
    (net / "tcp").write_text("header\n0: 00000000:2256 00000000:0000 0A 0:0 00:0 0 1000 0 999\n1: 0100007F:2256 00000000:0000 01 0:0 00:0 0 1000 0 998\n")
    (net / "tcp6").write_text("header\n0: 00000000000000000000000000000000:1FCB 00000000000000000000000000000000:0000 0A 0:0 00:0 0 1000 0 997\n")
    assert switch.listeners(tmp_path) == [{"address":"0.0.0.0","port":8790,"inode":"999"},{"address":"::","port":8139,"inode":"997"}]


def test_hidden_host_process_cannot_be_reported_as_stopped(tmp_path,monkeypatch):
    (tmp_path / "service.lock").write_text("1032694")
    monkeypatch.setattr(switch,"DATA",tmp_path)
    with (tmp_path / "service.lock").open("r") as active:
        switch.fcntl.flock(active, switch.fcntl.LOCK_EX | switch.fcntl.LOCK_NB)
        with pytest.raises(RuntimeError,match="看不到"): switch.verify_process_visibility({})
        switch.verify_process_visibility({1032694:{"owner":"new"}})
