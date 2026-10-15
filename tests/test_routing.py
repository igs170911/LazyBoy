"""Exercise product admission, native wire payloads and fallback ownership."""
import asyncio
import copy
import importlib
import json
from types import SimpleNamespace

import httpx
import pytest

from server.computers import Computers
from server.config import Settings
from server.routing import Router, deterministic_intent, routed_payload
from server.runtime import Runtime
from server.store import Store, uid


@pytest.fixture
def product(tmp_path):
    store = Store(tmp_path)
    settings = Settings(store)
    settings.update({"active":"primary","model":"work","provider":{"id":"primary","base_url":"https://primary.example/v1","api_key":"PRIMARY_PRIVATE", "models":[{"id":name,"protocol":"chat_completions","reasoning_effort":"low" if name=="fast" else "high"} for name in ("fast","work","backup")]}})
    settings.update({"routing":{"chat":{"provider":"primary","model":"fast"},"work":{"provider":"primary","model":"work"},"fallbacks":[{"provider":"primary","model":"backup"}]}})
    computers = Computers(store, settings)
    runtime = Runtime(store, settings, computers)
    runtime.closed = True
    agent = {"id":uid(), "name":"Routing fixture"}
    store.put("agents", agent["id"], agent)
    yield SimpleNamespace(store=store,settings=settings,computers=computers,runtime=runtime,agent=agent)
    store.conn.close()


async def start_route(product, text="你好", **kwargs):
    task_id = (await product.runtime.submit(product.agent["id"],text,"one-admission"))["queued"]
    route = await product.runtime.router.select(product.agent,text,**kwargs)
    product.runtime.active[product.agent["id"]] = task_id
    product.runtime.routes[task_id] = route
    product.runtime.provider_bindings[task_id] = route.primary()
    product.store.patch("tasks",task_id,state="running")
    return task_id, route


@pytest.mark.parametrize("text,intent", [("你好","answer"),("幫我搜尋今天新聞","native"),("瀏覽器登入","browser"),("操作桌面","desktop"),("完整實作功能","work"),("整理我的想法",None)])
def test_deterministic_hints(text,intent):
    assert deterministic_intent(text) == intent


async def test_role_selection_pins_routines_and_continuations(product):
    router = product.runtime.router
    assert (await router.select(product.agent,"你好")).primary()[1] == "fast"
    for text, kwargs in (("完整實作",{}),("繼續",{}),("你好",{"routine":True}),("你好",{"vision":True})):
        assert (await router.select(product.agent,text,**kwargs)).primary()[1] == "work"
    for pinned in ({"model":"backup"},{"provider":"primary","model":"backup"}):
        route = await router.select({**product.agent,**pinned},"你好")
        assert not route.enabled and route.primary()[1] == "backup"
    product.settings.update({"routing":{"enabled":False,"chat":{"provider":"primary","model":"fast"}}})
    route = await router.select(product.agent,"你好")
    assert not route.enabled and route.primary()[1] == "work"


def test_routing_validation_is_atomic_and_keeps_credentials_private(product):
    before = product.settings.path.read_bytes()
    for patch in ({"unknown":True},{"min_confidence":float("nan")},{"classifier_url":"https://remote.example"},{"classifier_timeout_ms":0},{"work":{"provider":"primary","model":"missing"}},{"fallbacks":[{}]*4}):
        with pytest.raises(ValueError): product.settings.update({"routing":patch})
        assert product.settings.path.read_bytes() == before
    public = product.settings.public()
    assert public["routing"]["chat"]["model"] == "fast"
    assert "PRIMARY_PRIVATE" not in json.dumps(public)
    product.settings.update({"provider":{"id":"mixed","base_url":"https://mixed.example/v1","models":[{"id":"other","protocol":"messages"}]}})
    before = product.settings.path.read_bytes()
    with pytest.raises(ValueError,match="相同 API"):
        product.settings.update({"routing":{"chat":{"provider":"primary","model":"fast"},"work":{"provider":"mixed","model":"other"}}})
    assert product.settings.path.read_bytes() == before


async def test_classifier_confidence_abstention_deadline_and_circuit(product,monkeypatch):
    routing = {**product.settings.public()["routing"],"semantic":True,"classifier_shadow":False,"classifier_timeout_ms":20}
    product.settings.update({"routing":routing})
    seen=[]; mode="confident"
    async def upstream(request):
        seen.append(request)
        if mode=="timeout": await asyncio.sleep(.2)
        if mode=="failure": raise httpx.ConnectError("unavailable")
        return httpx.Response(200,json={"route":"answer","confidence":.98 if mode=="confident" else .1,"abstained":mode=="abstain"})
    original=httpx.AsyncClient
    monkeypatch.setattr(httpx,"AsyncClient",lambda **kwargs:original(transport=httpx.MockTransport(upstream),**kwargs))
    router=product.runtime.router
    assert (await router.select(product.agent,"Explain supplied text")).role=="chat"
    for value in ("low","abstain","timeout"):
        mode=value
        assert (await router.select(product.agent,"Explain supplied text")).role=="work"
    assert len(seen)==4
    mode="confident"
    assert (await router.select(product.agent,"Explain supplied text")).role=="work"
    assert len(seen)==4  # Failed transport is cooling down, not low confidence.
    await router.close()


async def test_shadow_does_not_hold_admission_or_change_role(product,monkeypatch):
    product.settings.update({"routing":{**product.settings.public()["routing"],"laya":True,"classifier_shadow":True}})
    entered=asyncio.Event(); release=asyncio.Event(); events=[]
    async def classify(*args): entered.set(); await release.wait(); return "work"
    monkeypatch.setattr(product.runtime.router,"classify",classify)
    route=await product.runtime.router.select(product.agent,"Explain supplied text",emit=lambda kind,payload:events.append((kind,payload)))
    assert route.role=="chat"
    await entered.wait(); release.set()
    await asyncio.gather(*product.runtime.router.observers)
    assert events[-1][0]=="classifier_shadow" and events[-1][1]["suggested"]=="work"
    assert route.role=="chat"


async def test_turn_snapshot_and_worker_boot_use_selected_model(product):
    _,route=await start_route(product)
    product.computers.prepare(product.agent["id"])
    import yaml
    config=yaml.safe_load((product.store.agent_directory(product.agent["id"])/"hermes/config.yaml").read_text())
    assert config["model"]["default"]=="fast"
    product.settings.update({"provider":{"id":"primary","api_key":"NEW_PRIVATE"}})
    assert route.primary()[2]["api_key"]=="PRIMARY_PRIVATE"


async def test_actual_task_queue_passes_selected_model_to_native_session(product,monkeypatch):
    runtime=product.runtime
    response=await runtime.submit(product.agent["id"],"你好","native-admission")
    calls=[]
    class Client:
        alive=True
        product_session="native-session"
        async def call(self,method,params):
            calls.append((method,params))
            if method=="prompt.submit": runtime.finish(product.agent["id"],response["queued"],"answer","Hello")
            return {"status":"accepted"}
    client=Client()
    async def connected(id): return client
    monkeypatch.setattr(runtime,"client",connected)
    runtime.closed=False
    await asyncio.wait_for(runtime._queue(product.agent["id"]),1)
    assert ("config.set",{"session_id":"native-session","key":"model","value":"fast --provider custom:product","confirm_expensive_model":True}) in calls
    task=runtime.task(response["queued"])
    assert task["state"]=="terminal" and task["model"]=="fast"
    assert product.settings.effective()[1]=="work"
    assert not runtime.routes and not runtime.provider_bindings


def protocol_payload(protocol, model="fast"):
    if protocol=="chat_completions":
        return {"model":model,"tools":[{"type":"function","function":{"name":"terminal"}}],"messages":[{"role":"assistant","content":"","reasoning_content":"PRIVATE_REASONING","tool_calls":[{"id":"call-1","function":{"name":"terminal","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call-1","content":"VERIFIED"}]}
    if protocol=="responses":
        return {"model":model,"tools":[{"type":"function","name":"terminal"}],"input":[{"type":"reasoning","encrypted_content":"PRIVATE_REASONING"},{"type":"function_call","call_id":"call-1","name":"terminal","arguments":"{}"},{"type":"function_call_output","call_id":"call-1","output":"VERIFIED"}]}
    return {"model":model,"tools":[{"name":"terminal","input_schema":{}}],"max_tokens":20000,"messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"PRIVATE_REASONING","signature":"opaque"},{"type":"tool_use","id":"call-1","name":"terminal","input":{}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"call-1","content":"VERIFIED"}]}]}


@pytest.mark.parametrize("protocol",["chat_completions","responses","messages"])
async def test_real_proxy_promotes_after_tool_and_preserves_pairs(product,monkeypatch,protocol):
    config=product.settings.read()
    for row in config["providers"]["primary"]["models"]: row["protocol"]=protocol
    product.settings.save(config)
    task,route=await start_route(product)
    main=importlib.import_module("server.main")
    for name in ("store","settings","computers","runtime"): monkeypatch.setattr(main,name,getattr(product,name))
    seen=[]; original=httpx.AsyncClient
    def upstream(request): seen.append(request); return httpx.Response(200,json={"ok":True})
    client=original(transport=httpx.ASGITransport(app=main.app),base_url="http://localhost")
    monkeypatch.setattr(httpx,"AsyncClient",lambda **kwargs:original(transport=httpx.MockTransport(upstream),**kwargs))
    path={"chat_completions":"chat/completions","responses":"responses","messages":"messages"}[protocol]
    payload=protocol_payload(protocol)
    async with client:
        response=await client.post("/internal/providers/primary/v1/"+path,headers={"Authorization":"Bearer "+product.computers.key(product.agent["id"])},json=payload)
        assert response.status_code==200 and json.loads(seen[-1].content)["model"]=="fast"
        await product.runtime.event(product.agent["id"],{"type":"tool.start","payload":{"name":"terminal","tool_id":"call-1"}})
        response=await client.post("/internal/providers/primary/v1/"+path,headers={"Authorization":"Bearer "+product.computers.key(product.agent["id"])},json=payload)
    assert response.status_code==200 and route.role=="work"
    wire=json.loads(seen[-1].content)
    assert wire["model"]=="work" and "VERIFIED" in json.dumps(wire) and "call-1" in json.dumps(wire)
    assert "PRIVATE_REASONING" not in json.dumps(wire)
    assert product.runtime.task(task)["model_requests"][-1]["role"]=="work"
    assert "PRIMARY_PRIVATE" not in json.dumps(product.store.events(product.agent["id"]))
    assert payload==protocol_payload(protocol)


@pytest.mark.parametrize("failure",[429,503,"connection",400])
async def test_fallback_has_own_headers_and_no_retry_on_permanent_error(product,monkeypatch,failure):
    product.settings.update({"provider":{"id":"alternate","base_url":"https://alternate.example/v1","api_key":"ALTERNATE_PRIVATE","models":[{"id":"work","reasoning_effort":"high"}]},"routing":{"chat":{"provider":"primary","model":"fast"},"work":{"provider":"alternate","model":"work"}}})
    task,route=await start_route(product)
    main=importlib.import_module("server.main")
    for name in ("store","settings","computers","runtime"): monkeypatch.setattr(main,name,getattr(product,name))
    seen=[]; original=httpx.AsyncClient
    def upstream(request):
        seen.append(request)
        if len(seen)==1:
            if failure=="connection": raise httpx.ConnectError("failure")
            return httpx.Response(failure,json={"error":"fixture"})
        return httpx.Response(200,json={"ok":True})
    client=original(transport=httpx.ASGITransport(app=main.app),base_url="http://localhost")
    monkeypatch.setattr(httpx,"AsyncClient",lambda **kwargs:original(transport=httpx.MockTransport(upstream),**kwargs))
    async with client:
        response=await client.post("/internal/providers/primary/v1/chat/completions",headers={"Authorization":"Bearer "+product.computers.key(product.agent["id"])},json=protocol_payload("chat_completions"))
    if failure==400:
        assert response.status_code==400 and len(seen)==1
    else:
        assert response.status_code==200 and len(seen)==2
        assert str(seen[-1].url).startswith("https://alternate.example/")
        assert seen[-1].headers["authorization"]=="Bearer ALTERNATE_PRIVATE"
        assert json.loads(seen[-1].content)["reasoning_effort"]=="high"
        assert route.last[0]=="alternate" and product.runtime.task(task)["provider"]=="alternate"


async def test_partial_stream_never_falls_back(product,monkeypatch):
    await start_route(product)
    main=importlib.import_module("server.main")
    for name in ("store","settings","computers","runtime"): monkeypatch.setattr(main,name,getattr(product,name))
    seen=[]; original=httpx.AsyncClient
    class Partial(httpx.AsyncByteStream):
        async def __aiter__(self):
            yield b'data: {"choices":[{"delta":{"content":"started"}}]}\n\n'
            raise httpx.ReadError("cut stream")
    def upstream(request): seen.append(request); return httpx.Response(200,stream=Partial(),headers={"content-type":"text/event-stream"})
    client=original(transport=httpx.ASGITransport(app=main.app),base_url="http://localhost")
    monkeypatch.setattr(httpx,"AsyncClient",lambda **kwargs:original(transport=httpx.MockTransport(upstream),**kwargs))
    async with client:
        with pytest.raises((httpx.ReadError, RuntimeError)) as error:
            await client.post("/internal/providers/primary/v1/chat/completions",headers={"Authorization":"Bearer "+product.computers.key(product.agent["id"])},json=protocol_payload("chat_completions"))
        cause = error.value
        while cause.__cause__ is not None: cause = cause.__cause__
        assert isinstance(cause, httpx.ReadError)
    assert len(seen)==1


@pytest.mark.parametrize("model,effort,expected",[("claude-opus-4-6","xhigh","max"),("claude-opus-4-7","high","high"),("claude-sonnet-4-5","low",None)])
def test_messages_model_specific_thinking(product,model,effort,expected):
    provider={"models":[{"id":model,"reasoning_effort":effort}]}
    payload=protocol_payload("messages")
    payload["temperature"]=.3
    wire=routed_payload(payload,("primary",model,provider),"messages",product.settings)
    if expected:
        assert wire["thinking"]["type"]=="adaptive" and wire["output_config"]["effort"]==expected
        if "4-7" in model: assert "temperature" not in wire
    else:
        assert wire["thinking"]["budget_tokens"]==4000 and wire["temperature"]==1


@pytest.mark.parametrize("protocol",["chat_completions","responses","messages"])
async def test_tool_history_promotes_without_waiting_for_ws_and_ignores_prior_turns(product,protocol):
    route=await product.runtime.router.select(product.agent,"你好")
    payload=protocol_payload(protocol)
    field="input" if protocol=="responses" else "messages"
    old=copy.deepcopy(payload[field])
    user={"role":"user","content":"你好" if protocol=="chat_completions" else [{"type":"input_text" if protocol=="responses" else "text","text":"你好"}]}
    payload[field]=old+[user]
    assert not route.executed_tools(payload)
    payload[field].extend(old)
    assert route.executed_tools(payload)


@pytest.mark.parametrize("protocol",["chat_completions","responses","messages"])
async def test_orphan_tool_results_and_bookkeeping_do_not_promote(product,protocol):
    route=await product.runtime.router.select(product.agent,"你好")
    payload=protocol_payload(protocol)
    field="input" if protocol=="responses" else "messages"
    pair=payload[field][-2:]
    user={"role":"user","content":"你好"}
    payload[field]=[user,pair[-1]]
    assert not route.executed_tools(payload)
    if protocol=="chat_completions": pair[0]["tool_calls"][0]["function"]["name"]="todo"
    elif protocol=="responses": pair[0]["name"]="todo"
    else: pair[0]["content"][-1]["name"]="todo"
    payload[field]=[user,*pair]
    assert not route.executed_tools(payload)


def test_responses_switch_discards_opaque_compaction_and_previous_effort(product):
    payload=protocol_payload("responses")
    payload["reasoning"]={"effort":"high","summary":"auto"}
    payload["input"].insert(1,{"type":"compaction","encrypted_content":"OTHER_MODEL_COMPACTION"})
    provider={"models":[{"id":"work"}]}
    wire=routed_payload(payload,("primary","work",provider),"responses",product.settings)
    assert "reasoning" not in wire and "OTHER_MODEL_COMPACTION" not in json.dumps(wire)
    assert [item["type"] for item in wire["input"]]==["function_call","function_call_output"]
    provider["models"][0]["reasoning_effort"]="low"
    wire=routed_payload(payload,("primary","work",provider),"responses",product.settings)
    assert wire["reasoning"]=={"effort":"low"}


async def test_stream_failure_before_first_byte_can_use_fallback(product,monkeypatch):
    await start_route(product)
    main=importlib.import_module("server.main")
    for name in ("store","settings","computers","runtime"): monkeypatch.setattr(main,name,getattr(product,name))
    seen=[];original=httpx.AsyncClient
    class EmptyFailure(httpx.AsyncByteStream):
        async def __aiter__(self):
            raise httpx.ReadError("failed before output")
            yield b""
    def upstream(request):
        seen.append(request)
        return httpx.Response(200,stream=EmptyFailure(),headers={"content-type":"text/event-stream"}) if len(seen)==1 else httpx.Response(200,json={"ok":True})
    client=original(transport=httpx.ASGITransport(app=main.app),base_url="http://localhost")
    monkeypatch.setattr(httpx,"AsyncClient",lambda **kwargs:original(transport=httpx.MockTransport(upstream),**kwargs))
    async with client:
        result=await client.post("/internal/providers/primary/v1/chat/completions",headers={"Authorization":"Bearer "+product.computers.key(product.agent["id"])},json=protocol_payload("chat_completions"))
    assert result.status_code==200 and [json.loads(r.content)["model"] for r in seen]==["fast","work"]


async def test_stop_cancels_waiting_first_byte_without_starting_fallback(product,monkeypatch):
    task,_=await start_route(product)
    main=importlib.import_module("server.main")
    for name in ("store","settings","computers","runtime"): monkeypatch.setattr(main,name,getattr(product,name))
    entered=asyncio.Event();cancelled=asyncio.Event();seen=[];original=httpx.AsyncClient
    class Waiting(httpx.AsyncByteStream):
        async def __aiter__(self):
            entered.set()
            try: await asyncio.Event().wait()
            finally: cancelled.set()
            yield b"unreachable"
    def upstream(request): seen.append(request);return httpx.Response(200,stream=Waiting(),headers={"content-type":"text/event-stream"})
    client=original(transport=httpx.ASGITransport(app=main.app),base_url="http://localhost")
    monkeypatch.setattr(httpx,"AsyncClient",lambda **kwargs:original(transport=httpx.MockTransport(upstream),**kwargs))
    async with client:
        posting=asyncio.create_task(client.post("/internal/providers/primary/v1/chat/completions",headers={"Authorization":"Bearer "+product.computers.key(product.agent["id"])},json=protocol_payload("chat_completions")))
        await entered.wait()
        await product.runtime.stop_task(product.agent["id"],task)
        response=await asyncio.wait_for(posting,1)
    assert response.status_code==400 and cancelled.is_set() and len(seen)==1


async def test_a_stop_does_not_cut_off_delegated_children(product,monkeypatch):
    """A stopped turn's own requests are refused; a delegate_task child (maybe of an earlier task) still gets through."""
    task,route=await start_route(product)
    main=importlib.import_module("server.main")
    for name in ("store","settings","computers","runtime"): monkeypatch.setattr(main,name,getattr(product,name))
    original=httpx.AsyncClient
    monkeypatch.setattr(httpx,"AsyncClient",lambda **kwargs:original(transport=httpx.MockTransport(lambda request: httpx.Response(200,json={"ok":True})),**kwargs))
    product.store.patch("tasks",task,state="stopping")
    headers={"Authorization":"Bearer "+product.computers.key(product.agent["id"])}
    own=protocol_payload("chat_completions")
    child={**own,"messages":[{"role":"system","content":main.CHILD_PROMPT+"\nCONTEXT: compare prices"},*own["messages"]]}
    before=len(product.runtime.task(task).get("model_requests",[]))
    async with original(transport=httpx.ASGITransport(app=main.app),base_url="http://localhost") as client:
        assert (await client.post("/internal/providers/primary/v1/chat/completions",headers=headers,json=own)).status_code==400
        assert (await client.post("/internal/providers/primary/v1/chat/completions",headers=headers,json=child)).status_code==200
    assert len(product.runtime.task(task).get("model_requests",[]))==before
    assert main.delegated_child({"system":main.CHILD_PROMPT}) and main.delegated_child({"instructions":main.CHILD_PROMPT})
    assert not main.delegated_child(own) and not main.delegated_child(None)


async def test_a_turn_that_never_starts_answering_is_a_gateway_timeout(product,monkeypatch):
    main=importlib.import_module("server.main")
    for name in ("store","settings","computers","runtime"): monkeypatch.setattr(main,name,getattr(product,name))
    original=httpx.AsyncClient
    def silent(request): raise TimeoutError("first byte never came")
    monkeypatch.setattr(httpx,"AsyncClient",lambda **kwargs:original(transport=httpx.MockTransport(silent),**kwargs))
    headers={"Authorization":"Bearer "+product.computers.key(product.agent["id"])}
    async with original(transport=httpx.ASGITransport(app=main.app),base_url="http://localhost") as client:
        response=await client.post("/internal/providers/primary/v1/chat/completions",headers=headers,json={"model":"fast","messages":[]})
    assert response.status_code==502 and "error" in response.json()
