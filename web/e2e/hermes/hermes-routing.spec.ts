import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { resolve, extname } from "node:path";
import { DEFAULT_ROUTING } from "../../src/routing-settings";

// Serve the actual production build through browser interception. No test port,
// product data or model service is needed.
test("Hermes routing saves independently, retries errors and preserves model drafts", async ({page}) => {
  const settings: any = {engine:"hermes",active:"fixture",model:"work",protocol:"chat_completions",base_url:"https://fixture.example/v1",providers:[{id:"fixture",label:"Fixture",base_url:"https://fixture.example/v1",has_key:true,models:[{id:"fast:latest"},{id:"work"},{id:"backup"}]}],routing:{...DEFAULT_ROUTING}};
  const writes: any[]=[]; let fail=true;
  await page.addInitScript(() => {
    localStorage.setItem("lazyhermes.locale","zh-Hant");
    (window as any).EventSource=class{close(){}};
  });
  await page.route("**/*",async route => {
    const request=route.request(), url=new URL(request.url()),path=url.pathname;
    if(path.startsWith("/api/")) {
      let body: any={};
      if(path==="/api/auth/status") body={authenticated:true,local:true};
      else if(path==="/api/settings") {
        if(request.method()==="PUT") {
          writes.push(request.postDataJSON());
          if(fail){fail=false;await route.fulfill({status:503,json:{error:"暫時無法儲存"}});return;}
          settings.routing=writes.at(-1).routing;
        }
        body=settings;
      } else if(path==="/api/settings/routing/health") body={ready:true,supported:true,engines:{semantic:"ready",laya:"ready"}};
      else if(path==="/api/providers/models") body={models:["fast:latest","work","backup"]};
      else if(path==="/api/agents") body={agents:[],groups:[]};
      await route.fulfill({json:body});return;
    }
    const root=resolve("dist"),file=resolve(root,path==="/"?"index.html":"."+path);
    if(!file.startsWith(root+"/")){await route.fulfill({status:404});return;}
    try {
      const contentType=({".html":"text/html",".js":"application/javascript",".css":"text/css",".woff2":"font/woff2",".svg":"image/svg+xml"} as Record<string,string>)[extname(file)] || "application/octet-stream";
      await route.fulfill({body:await readFile(file),contentType});
    } catch {await route.fulfill({status:404});}
  });
  await page.goto("http://localhost/");
  await page.getByRole("button",{name:"設定",exact:true}).click();
  if((page.viewportSize()?.width || 1280)>700) await page.getByRole("button",{name:"模型",exact:true}).click();
  const panel=page.getByRole("region",{name:"對話與工作分流"});
  const base=page.getByLabel("API 網址",{exact:true});
  await base.fill("https://unsaved.example/v1");
  await panel.getByLabel("聊天模型",{exact:true}).selectOption(JSON.stringify(["fixture","fast:latest"]));
  await panel.getByLabel("聊天模型 · 推理強度").selectOption("low");
  await panel.getByLabel("工作模型",{exact:true}).selectOption(JSON.stringify(["fixture","work"]));
  await panel.getByLabel("工作模型 · 推理強度").selectOption("high");
  await panel.locator("summary").click();
  await panel.getByLabel("備援模型 1",{exact:true}).selectOption(JSON.stringify(["fixture","backup"]));
  await panel.getByLabel("本機意圖判斷 URL").fill("http://127.0.0.1:8778");
  await panel.getByLabel("Laya 多語模型",{exact:true}).check();
  await panel.getByRole("button",{name:"儲存分流設定",exact:true}).click();
  await expect(panel.getByRole("alert")).toHaveText("暫時無法儲存");
  await expect(panel.getByLabel("聊天模型 · 推理強度")).toHaveValue("low");
  await panel.getByRole("button",{name:"儲存分流設定",exact:true}).click();
  await expect.poll(()=>writes.length).toBe(2);
  expect(writes[1]).toMatchObject({routing:{chat:{model:"fast:latest",reasoning_effort:"low"},work:{model:"work",reasoning_effort:"high"},fallbacks:[{model:"backup"}],laya:true}});
  expect(writes[1].provider).toBeUndefined();
  await expect(base).toHaveValue("https://unsaved.example/v1");
});
