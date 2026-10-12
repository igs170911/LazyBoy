#!/usr/bin/env python3
"""Interleave equal-model product tasks; verify actual artifacts, not prose."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path, PurePosixPath
import statistics
import time
import uuid

import httpx


DEFAULT_CASES=[{"id":"file","prompt":"建立 /workspace/comparison.csv，內容必須正好為 name,value\\nproof,7\\n（\\n 表示換行），實際讀回核對後再回答。","path":"comparison.csv","expected":"name,value\nproof,7\n"}]


def summarize(rows):
    result={}
    for engine in sorted({r["engine"] for r in rows}):
        selected=[r for r in rows if r["engine"]==engine]
        warm=[r["elapsed_ms"] for r in selected if r["verified"] and not r["cold_worker"]]
        result[engine]={"samples":len(selected),"verified":sum(r["verified"] for r in selected),"warm_verified_samples":len(warm),"warm_median_ms":statistics.median(warm) if warm else None}
    return result


def model_entry(settings,provider,model):
    selected=next((p for p in settings["providers"] if p["id"]==provider),None)
    row=next((m for m in selected.get("models",[]) if m["id"]==model),None) if selected else None
    if not row or not selected.get("base_url"): raise ValueError("兩個產品都必須先設定指定的供應商及模型")
    if not row.get("reasoning_effort"): raise ValueError("比較前須在兩邊明確設定相同推理強度，避免預設值不同")
    return selected["base_url"].rstrip("/"),row.get("protocol","chat_completions"),row["reasoning_effort"]


def compare(urls,provider,model,cases,repeats=5,timeout=300,client_factory=httpx.Client,prepare=None):
    if len(urls)!=2 or len(set(urls.values()))!=2: raise ValueError("比較必須使用兩個獨立產品網址")
    clients={};owned={};owned_names={};rows=[];cleanup_failures=[]
    try:
        identities=[]
        for engine,url in urls.items():
            client=client_factory(base_url=url,timeout=30);clients[engine]=client
            response=client.get("/api/settings");response.raise_for_status()
            identities.append(model_entry(response.json(),provider,model))
        if len(set(identities))!=1: raise ValueError("兩邊的模型網址、API 介面及推理強度須相同")
        for engine,client in clients.items():
            response=client.get("/api/agents");response.raise_for_status()
            existing={r["id"] for r in response.json()["agents"]}
            name="comparison-"+str(uuid.uuid4())
            response=client.post("/api/agents",json={"name":name,"provider":provider,"model":model});response.raise_for_status()
            agent=response.json()["id"]
            uuid.UUID(agent)
            if agent in existing: raise ValueError("產品未建立獨立比較 Bot")
            owned[engine]=agent
            owned_names[engine]=name
            response=client.post(f"/api/agents/{agent}/computer",json={"action":"start"});response.raise_for_status()
            if not response.json().get("ready"): raise ValueError("比較電腦尚未就緒")
        if prepare: prepare({engine:{"id":agent,"name":owned_names[engine]} for engine,agent in owned.items()})
        names=list(clients)
        for index in range(repeats):
            order=names if index%2==0 else list(reversed(names))
            for case in cases:
                path=PurePosixPath(case["path"])
                fresh_path=str(path.with_name(path.stem+f"-trial{index}"+path.suffix))
                prompt=case["prompt"].replace(case["path"],fresh_path)
                for engine in order:
                    client=clients[engine];agent=owned[engine]
                    response=client.delete(f"/api/agents/{agent}/conversation");response.raise_for_status()
                    started=time.monotonic();task={};verified=False;error=None
                    try:
                        response=client.post(f"/api/agents/{agent}/messages",json={"text":prompt,"request_id":str(uuid.uuid4())});response.raise_for_status()
                        task_id=response.json()["queued"]
                        while True:
                            response=client.get("/api/tasks/"+task_id);response.raise_for_status();task=response.json()
                            if task["state"]=="terminal": break
                            if time.monotonic()-started>timeout:
                                client.post(f"/api/agents/{agent}/stop").raise_for_status()
                                raise TimeoutError("comparison deadline")
                            time.sleep(.25)
                        artifact=client.get(f"/api/agents/{agent}/files/content",params={"path":fresh_path})
                        verified=artifact.is_success and artifact.text==case["expected"] and task.get("verdict") not in {"failed","cancelled","interrupted","blocked"}
                        if task.get("model") and task["model"]!=model: verified=False;error="model_mismatch"
                    except (httpx.HTTPError,TimeoutError,KeyError) as exc: error=type(exc).__name__
                    rows.append({"engine":engine,"case":case["id"],"sample":index,"cold_worker":not any(r["engine"]==engine for r in rows),"elapsed_ms":round((time.monotonic()-started)*1000,2),"verified":verified,"verdict":task.get("verdict"),"model_from_task":task.get("model"),"model_requests":len(task["model_requests"]) if "model_requests" in task else None,"budget_requests":task.get("requests"),"usage":{k:v for k,v in (task.get("usage") or {}).items() if k in {"input_tokens","output_tokens","prompt_tokens","completion_tokens","total_tokens"}},"error":error})
        return {"date":datetime.now(timezone.utc).isoformat(),"method":"Equal endpoint/model/protocol/explicit reasoning; ready independent computers; interleaved order; exact downloaded artifact verification.","model":model,"protocol":identities[0][1],"reasoning_effort":identities[0][2],"scope":"Only the supplied cases. No general website or p95 claim.","rows":rows,"summary":summarize(rows),"cleanup_failures":cleanup_failures}
    finally:
        for engine,agent in owned.items():
            try:
                clients[engine].post(f"/api/agents/{agent}/stop").raise_for_status()
                clients[engine].delete(f"/api/agents/{agent}").raise_for_status()
            except httpx.HTTPError: cleanup_failures.append(engine)
        for client in clients.values(): client.close()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lazyboy-url",default="http://127.0.0.1:8787")
    parser.add_argument("--hermes-url",default="http://127.0.0.1:8790")
    parser.add_argument("--provider",required=True);parser.add_argument("--model",required=True)
    parser.add_argument("--cases",type=Path,help="JSON cases with id/prompt/path/expected; browser/login cases require equally prepared account conditions")
    parser.add_argument("--samples",type=int,default=5)
    parser.add_argument("--timeout",type=int,default=300)
    parser.add_argument("--output",type=Path,default=Path(".run/comparison.json"))
    parser.add_argument("--live",action="store_true",help="Execute real configured models, which may incur API charges")
    parser.add_argument("--same-account",action="store_true",help="Declare both services use the same model account")
    parser.add_argument("--prepare-browser",action="store_true",help="Pause after creating computers so you can log in to both benchmark Bots")
    args=parser.parse_args()
    cases=json.loads(args.cases.read_text()) if args.cases else DEFAULT_CASES
    if not isinstance(cases,list) or not cases or any(not isinstance(c,dict) or any(not isinstance(c.get(k),str) for k in ("id","prompt","path","expected")) for c in cases): parser.error("無效的比較案例")
    if any(not c["path"] or c["path"] not in c["prompt"] or PurePosixPath(c["path"]).is_absolute() or ".." in PurePosixPath(c["path"]).parts for c in cases): parser.error("案例的成果相對路徑須出現在指令中，不可包含 ..")
    if args.samples<5 or args.timeout<=0: parser.error("每個案例至少五次，等待上限須大於零")
    if not args.live:
        print(json.dumps({"live":False,"provider":args.provider,"model":args.model,"samples_per_engine_per_case":args.samples,"cases":[c["id"] for c in cases]},ensure_ascii=False,indent=2));return
    if not args.same_account: parser.error("請先核對兩邊模型帳號，再指定 --same-account")
    if any(case.get("requires_login") for case in cases) and not args.prepare_browser: parser.error("登入案例須指定 --prepare-browser，先在兩個比較 Bot 登入")
    urls={"lazyboy":args.lazyboy_url,"hermes":args.hermes_url}
    def prepare(owned):
        for engine,agent in owned.items(): print(f"{engine}: {urls[engine]} · {agent['name']} ({agent['id']})")
        input("在兩個比較 Bot 的電腦完成相同登入與頁面準備後，按 Enter 開始：")
    report=compare(urls,args.provider,args.model,cases,args.samples,args.timeout,prepare=prepare if args.prepare_browser else None)
    report["account_match"]="declared by operator"
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2)+"\n")
    print(str(args.output))
    return 0 if all(row["verified"] for row in report["rows"]) and not report["cleanup_failures"] else 1


if __name__=="__main__": raise SystemExit(main() or 0)
