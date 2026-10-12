#!/usr/bin/env python3
"""Evaluate labeled JSONL without retaining private prompts in the report.
Synthetic cases are infrastructure checks, not measured user-dialogue quality.
"""
import argparse
import collections
import json
from pathlib import Path
import statistics
import time
import urllib.error
import urllib.request


def health(url):
    # /health answers 503 while any engine is not ready; its body still reports busy.
    try:
        with urllib.request.urlopen(url+'/health',timeout=2) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        with error:
            return json.load(error)


def synthetic_cases():
    cases = {
        "answer": [
            "解釋為什麼天空看起來是藍色", "把『今天最新消息』這句話翻譯成英文", "解釋這份提供的登入說明", "用簡單例子說明函數", "將這段提供的內容改寫為禮貌訊息",
            "解釋瀏覽器和搜尋引擎有何不同", "替我想五個咖啡店名稱", "如何理解遞迴的概念", "把已貼上的文字縮短", "用英文說『謝謝你的協助』",
            "Explain why the sky looks blue", "Translate the phrase latest news today into Chinese", "Explain this supplied login guide", "Explain what a function means with an example", "Rewrite the supplied paragraph politely",
            "Explain the difference between a browser and a search engine", "Suggest five coffee shop names", "Explain recursion in plain language", "Shorten the text I pasted above", "Translate thank you for your help"],
        "native": [
            "找出今天台北的氣溫", "搜尋剛發布的官方通知", "讀取此專案的 README", "執行目前專案的測試", "搜尋 Google Drive 的預算試算表",
            "查詢這家公司最近的新聞", "列出工作目錄中的檔案", "讀取資料檔並計算總和", "搜尋官方軟體安裝文件", "查詢明天的天氣預報",
            "Look up the current temperature in Taipei", "Search for the newly released official announcement", "Read this project's README file", "Run the tests in this repository", "Find the budget spreadsheet in Google Drive",
            "Search recent news about this company", "List files in the working directory", "Read the data file and calculate its sum", "Find official software installation documentation", "Check tomorrow's weather forecast"],
        "browser": [
            "使用瀏覽器登入會員網站", "在已登入的網站上填寫表單", "打開網站的 JavaScript 選單", "操作網站上的日期選擇器", "切換瀏覽器中的分頁",
            "在網站內新增購物車品項", "從已登入的入口網站下載報表", "查看網頁上動態更新的內容", "操作瀏覽器網頁中的下拉選單", "開啟帳戶網站的個人資料頁",
            "Log in to the member website using the browser", "Fill out the form on the signed-in website", "Open the website's JavaScript menu", "Use the website's date picker", "Switch browser tabs",
            "Add an item to the website shopping cart", "Download the report from the signed-in web portal", "Inspect the dynamically rendered website content", "Select an option in the browser page dropdown", "Open the profile page in the account website"],
        "desktop": [
            "截取目前桌面的畫面", "點擊原生桌面程式的工具列", "操作螢幕上的繪圖應用程式", "查看桌面程式的對話框", "在原生檔案管理員中拖曳圖示",
            "操作桌面上的終端機視窗", "查看螢幕上的錯誤提示", "點選原生應用程式的儲存按鈕", "在桌面程式中調整視窗大小", "檢查目前電腦桌面的視窗",
            "Capture a screenshot of the current desktop", "Click the toolbar in the native desktop app", "Use the drawing application on the screen", "Inspect the native desktop application's dialog", "Drag the icon in the native file manager",
            "Interact with the desktop terminal window", "Read the error dialog on the desktop screen", "Click Save in the native application", "Resize the desktop application window", "Inspect the windows visible on the computer desktop"],
        "work": [
            "完整實作帳號管理功能並驗證", "重構資料庫介面並完成回歸測試", "深入研究多個架構並比較取捨", "設計並實作整個資料處理流程", "修復此專案的錯誤並核對結果",
            "實作帶驗證的複雜表單功能", "深入研究三種技術方案並交付報告", "重構所有路由程式並檢查相容性", "完整實作後端 API 與測試", "調查效能瓶頸並完成修正及驗證",
            "Implement the complete account management feature and verify it", "Refactor the database interface and complete regression tests", "Research multiple architectures and compare tradeoffs in depth", "Design and implement the full data processing pipeline", "Fix this project's bug and verify the result",
            "Implement the complex form validation feature", "Research three approaches deeply and deliver a report", "Refactor all routing code and check compatibility", "Implement the complete backend API and its tests", "Investigate the performance bottleneck and implement and verify the fix"],
    }
    for route, texts in cases.items():
        for index, text in enumerate(texts):
            lang = "zh" if index < 10 else "en"
            for variant in range(2):
                prefix = ("請協助：" if variant == 0 else "我的需求是：") if lang == "zh" else ("Please help: " if variant == 0 else "My request is: ")
                yield {"id":f"{route}-{index}-{variant}", "group":f"{route}-{index}", "language":lang, "text":prefix+text, "route":route, "synthetic":True}


def metrics(rows):
    accepted=[r for r in rows if not r["abstained"]]
    correct=sum(r["expected"]==r["route"] for r in accepted)
    by_class={}
    for label in ["answer","native","browser","desktop","work"]:
        predicted=[r for r in accepted if r["route"]==label]
        total=sum(r["expected"]==label for r in rows)
        true=sum(r["expected"]==label for r in predicted)
        by_class[label]={"precision":true/len(predicted) if predicted else None,"recall_with_abstention":true/total if total else None,"support":total}
    latencies=sorted(r["wall_ms"] for r in rows)
    return {"cases":len(rows),"accepted":len(accepted),"coverage":len(accepted)/len(rows),
        "accepted_precision":correct/len(accepted) if accepted else None,
        "required_tools_classified_as_answer":sum(r["expected"]!="answer" and r["route"]=="answer" for r in accepted),
        "p50_ms":statistics.median(latencies),"p95_ms":latencies[int((len(latencies)-1)*.95)],"by_class":by_class}


def main():
    p=argparse.ArgumentParser();p.add_argument('--input',type=Path);p.add_argument('--synthetic',action='store_true')
    p.add_argument('--write-fixture',type=Path);p.add_argument('--url',default='http://127.0.0.1:8768');p.add_argument('--output',type=Path,required=True)
    p.add_argument('--modes',nargs='+',default=['semantic','laya','combined']);p.add_argument('--deadline-ms',type=int,default=250)
    p.add_argument('--confidence',type=float,default=.9);p.add_argument('--language',choices=['en','zh'])
    args=p.parse_args()
    if args.synthetic:cases=list(synthetic_cases())
    elif args.input:cases=[json.loads(line) for line in args.input.read_text().splitlines() if line.strip()]
    else:p.error('provide --input labeled.jsonl or --synthetic')
    if args.write_fixture:args.write_fixture.write_text(''.join(json.dumps(c,ensure_ascii=False)+'\n' for c in cases))
    if args.language:cases=[c for c in cases if c.get('language')==args.language]
    report={"source":"synthetic infrastructure fixture" if args.synthetic else "user-supplied labeled corpus", "independent_user_quality_measurement":not args.synthetic,
        "confidence":args.confidence,"deadline_ms":args.deadline_ms,"modes":{},"rows":[]}
    for mode in args.modes:
        rows=[]
        for index, case in enumerate(cases):
            # A deadline does not cancel native inference; wait outside measured
            # requests so one timeout cannot pollute the next case with 'busy'.
            for _ in range(200):
                if not health(args.url).get('busy'):break
                time.sleep(.01)
            body={"text":case['text'],"semantic":mode!='laya',"laya":mode!='semantic',"min_confidence":args.confidence,"deadline_ms":args.deadline_ms}
            req=urllib.request.Request(args.url+'/v1/route',json.dumps(body).encode(),headers={'content-type':'application/json'})
            start=time.perf_counter();result=json.load(urllib.request.urlopen(req,timeout=3));wall=(time.perf_counter()-start)*1000
            row={"id":case['id'],"mode":mode,"expected":case['route'],"route":result.get('route'),"confidence":result.get('confidence'),
                "abstained":result.get('abstained',False),"reason":result.get('reason'),"source":result.get('source'),"wall_ms":round(wall,2)}
            rows.append(row)
        report['modes'][mode]=metrics(rows);report['rows'].extend(rows)
        print(mode,json.dumps(report['modes'][mode]),flush=True)
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')

if __name__=='__main__':main()
