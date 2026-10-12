import json
import uuid

import httpx
import pytest

from scripts.compare import compare, DEFAULT_CASES, summarize


def test_comparison_interleaves_equal_cases_and_deletes_only_owned_bots():
    owned={};deleted=[];order=[];paths=[]
    formal=str(uuid.uuid4())
    settings={"providers":[{"id":"fixture","base_url":"https://provider.example/v1","models":[{"id":"m","reasoning_effort":"high"}]}]}
    def api(request):
        host=request.url.host;path=request.url.path
        if path=="/api/settings": return httpx.Response(200,json=settings)
        if path=="/api/agents":
            if request.method=="GET": return httpx.Response(200,json={"agents":[{"id":formal}]})
            owned[host]=str(uuid.uuid4());return httpx.Response(200,json={"id":owned[host]})
        if path.endswith("/computer"): return httpx.Response(200,json={"ready":True})
        if path.endswith("/messages"):
            assert "comparison-trial" in json.loads(request.content)["text"]
            order.append(host);return httpx.Response(200,json={"queued":"task"})
        if path=="/api/tasks/task": return httpx.Response(200,json={"state":"terminal","verdict":"answer","model":"m"})
        if path.endswith("/files/content"):
            paths.append(request.url.params["path"])
            return httpx.Response(200,text=DEFAULT_CASES[0]["expected"])
        if request.method=="DELETE" and not path.endswith("/conversation"):
            assert path.endswith(owned[host]) and not path.endswith(formal)
            deleted.append(host)
        return httpx.Response(200,json={"ok":True})
    report=compare({"lazyboy":"http://rust.fixture","hermes":"http://hermes.fixture"},"fixture","m",DEFAULT_CASES,3,client_factory=lambda **kwargs:httpx.Client(transport=httpx.MockTransport(api),**kwargs))
    assert order==["rust.fixture","hermes.fixture","hermes.fixture","rust.fixture","rust.fixture","hermes.fixture"]
    assert len(deleted)==2 and not report["cleanup_failures"]
    assert all(r["verified"] for r in report["rows"])
    assert sum(r["cold_worker"] for r in report["rows"])==2
    assert report["summary"]["hermes"]["warm_verified_samples"]==2
    assert all(r["model_requests"] is None for r in report["rows"])
    assert paths==["comparison-trial0.csv"]*2+["comparison-trial1.csv"]*2+["comparison-trial2.csv"]*2


def test_summary_does_not_mix_cancelled_samples_with_success_latency():
    rows=[{"engine":"hermes","verified":False,"elapsed_ms":99999,"cold_worker":False},{"engine":"hermes","verified":True,"elapsed_ms":1000,"cold_worker":True},{"engine":"hermes","verified":True,"elapsed_ms":10,"cold_worker":False}]
    assert summarize(rows)["hermes"]=={"samples":3,"verified":2,"warm_verified_samples":1,"warm_median_ms":10}


def test_comparison_refuses_mismatched_model_conditions_before_creating_bots():
    writes=[]
    def api(request):
        if request.method!="GET": writes.append(request)
        return httpx.Response(200,json={"providers":[{"id":"fixture","base_url":"https://provider.example/v1","models":[{"id":"m","reasoning_effort":"high" if request.url.host=="rust.fixture" else "low"}]}]})
    with pytest.raises(ValueError,match="相同"):
        compare({"lazyboy":"http://rust.fixture","hermes":"http://hermes.fixture"},"fixture","m",DEFAULT_CASES,client_factory=lambda **kwargs:httpx.Client(transport=httpx.MockTransport(api),**kwargs))
    assert writes==[]
