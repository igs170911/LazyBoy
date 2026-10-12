import importlib
import json
from types import SimpleNamespace
import pytest
import httpx
from server.store import Store, now, uid
from server.config import Settings
from server.computers import Computers
from server.runtime import Runtime
from server.skills import Skills
from server.scheduling import Scheduler


@pytest.fixture
def service(tmp_path,monkeypatch):
    m = importlib.import_module("server.main")
    store=Store(tmp_path); settings=Settings(store); computers=Computers(store,settings); runtime=Runtime(store,settings,computers); runtime.closed=True
    for key,value in {"store":store,"settings":settings,"computers":computers,"runtime":runtime,"skills":Skills(store),"scheduler":Scheduler(store,runtime)}.items(): monkeypatch.setattr(m,key,value)
    return m


async def test_remote_login_and_cross_origin_block(service):
    transport=httpx.ASGITransport(app=service.app,client=("192.0.2.9",10000))
    async with httpx.AsyncClient(transport=transport,base_url="http://workstation:8790") as c:
        assert (await c.get("/api/settings")).status_code == 401
        assert (await c.post("/api/auth/login",json={"token":"wrong"})).status_code == 401
        result=await c.post("/api/auth/login",json={"token":service.settings.access_token})
        assert result.status_code == 200 and "HttpOnly" in result.headers["set-cookie"]
        assert (await c.get("/api/settings")).status_code == 200
        assert (await c.post("/api/agents",json={"name":"Bad"},headers={"Origin":"http://foreign.example"})).status_code == 401


async def test_agent_upload_template_and_idempotent_tasks(service):
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=service.app),base_url="http://localhost:8790") as c:
        a=(await c.post("/api/agents",json={"name":"Writer"})).json(); b=(await c.post("/api/agents",json={"name":"Reader"})).json()
        upload=(await c.post(f"/api/agents/{a['id']}/uploads?name=sample.txt",content=b"OWN_FILE")).json()
        assert (await c.get(upload["url"])).content == b"OWN_FILE"
        cross=upload["url"].replace(a["id"],b["id"])
        assert (await c.get(cross)).status_code == 404
        assert (await c.get(f"/api/agents/{a['id']}/files/content",params={"path":"../../settings.json"})).status_code == 400
        body={"text":"Work","request_id":"durable-one"}
        first=(await c.post(f"/api/agents/{a['id']}/messages",json=body)).json()
        second=(await c.post(f"/api/agents/{a['id']}/messages",json=body)).json()
        assert first["queued"] == second["queued"]
        assert len(service.store.transcript(a["id"])) == 1
        await c.post(f"/api/agents/{a['id']}/skills",json={"action":"save","name":"Write","instructions":"Verify output."})
        service.store.put("mcp",a["id"],{"private":{"command":"example","env":{"PRIVATE_KEY":"HIDDEN_SENTINEL"},"headers":{"Authorization":"HIDDEN_SENTINEL"}}})
        template=await c.get(f"/api/agents/{a['id']}/template")
        assert "HIDDEN_SENTINEL" not in template.text
        assert next(s for s in template.json()["skills"] if s["name"] == "Write")["instructions"] == "Verify output."


async def test_listing_skills_does_not_announce_a_change(service):
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=service.app),base_url="http://localhost:8790") as c:
        agent=(await c.post("/api/agents",json={"name":"Skills"})).json()
        listed=await c.post(f"/api/agents/{agent['id']}/skills",json={"action":"list"})
        assert listed.status_code==200
        assert [e for e in service.store.events(agent["id"]) if e["kind"]=="skills_changed"]==[]
        saved=await c.post(f"/api/agents/{agent['id']}/skills",json={"action":"save","name":"Write","instructions":"Verify output."})
        assert saved.status_code==200
        notices=[e for e in service.store.events(agent["id"]) if e["kind"]=="skills_changed"]
        # Every bot also carries the built-in grok-build skill.
        assert len(notices)==1 and notices[0]["payload"]=={"count":2,"changed":True}
        again=await c.post(f"/api/agents/{agent['id']}/skills",json={"action":"list"})
        assert again.status_code==200
        assert len([e for e in service.store.events(agent["id"]) if e["kind"]=="skills_changed"])==1


async def test_saved_credentials_are_never_public(service):
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=service.app),base_url="http://localhost:8790") as c:
        body={"active":"test","model":"model","provider":{"id":"test","base_url":"http://localhost:12345/v1","api_key":"SECRET_SENTINEL","models":[{"id":"model"}]}}
        response=await c.put("/api/settings",json=body)
        assert response.status_code == 200 and "SECRET_SENTINEL" not in response.text
        assert next(p for p in response.json()["providers"] if p["id"]=="test")["selectable"]
        assert service.settings.read()["providers"]["test"]["api_key"] == "SECRET_SENTINEL"
        body["provider"]["api_key"]=""
        await c.put("/api/settings",json=body)
        assert service.settings.read()["providers"]["test"]["api_key"] == "SECRET_SENTINEL"


async def test_save_login_answer_keeps_identifier(service):
    runtime, store = service.runtime, service.store
    agent, task = uid(), uid()
    store.put("agents", agent, {"id": agent, "name": "Vault"})
    store.put("tasks", task, {"id": task, "agent_id": agent, "state": "running", "created_at": now()})
    runtime.active[agent] = task
    sent = []
    async def answer(rid, result): sent.append(result)
    runtime.clients[agent] = SimpleNamespace(alive=True, answer=answer)
    await runtime.human_request(agent, {"id": "save-1", "method": "vault.save_login", "params": {"origin": "https://shop.example", "site": "shop.example"}})
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=service.app), base_url="http://localhost:8790") as client:
        response = await client.post(f"/api/agents/{agent}/secrets", json={"question_id": "save-1", "values": {"identifier": "ada", "password": "sentinel-not-real"}})
    assert response.status_code == 200
    payload = json.loads(sent[0]["value"])
    assert payload == {"identifier": "ada", "password": "sentinel-not-real"}
    assert "sentinel-not-real" not in json.dumps(store.transcript(agent))


async def test_media_follows_the_bot_model_and_reasoning_config(service):
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=service.app),base_url="http://localhost:8790") as c:
        models=[{"id":"text","vision":False},{"id":"vision","vision":True,"reasoning_effort":"high"}]
        result=await c.put("/api/settings",json={"active":"test","model":"text","provider":{"id":"test","base_url":"http://localhost:12345/v1","models":models}})
        assert result.status_code==200
        a=(await c.post("/api/agents",json={"name":"Vision Bot","provider":"test","model":"vision"})).json()
        assert not (await c.get("/api/media")).json()["capabilities"]["images"]
        assert (await c.get("/api/media",params={"agent_id":a["id"]})).json()["capabilities"]["images"]
        import yaml
        config=yaml.safe_load((service.store.agent_directory(a["id"])/"hermes/config.yaml").read_text())
        assert config["model"]["supports_vision"] is True
        assert config["agent"]["image_input_mode"]=="native"
        assert config["agent"]["reasoning_effort"]=="high"
        invalid=await c.put("/api/settings",json={"provider":{"id":"test","models":[{"id":"text","reasoning_effort":"typo"}]}})
        assert invalid.status_code==400
