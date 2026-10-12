import { test, expect } from '@playwright/test';
import { DEFAULT_ROUTING } from '../../src/routing-settings';

test('routing settings preserve model IDs, separate effort, fallback order and unsaved edits after errors', async ({page}) => {
  let writes: any[] = [], fail = true;
  const settings: any = {active:'fixture',model:'work',providers:[{id:'fixture',label:'Fixture',has_key:true,
    models:[{id:'fast:latest',reasoning_levels:['low','high']},{id:'work',reasoning_levels:['low','high']},{id:'backup'}]}],routing:{...DEFAULT_ROUTING}};
  await page.addInitScript(() => {localStorage.setItem('lazyboy.locale','zh-Hant');(window as any).EventSource=class{close(){}};});
  await page.route('**/api/**',async route => {
    const req=route.request(),path=new URL(req.url()).pathname;
    if(path==='/api/auth/status')await route.fulfill({json:{authenticated:true}});
    else if(path==='/api/settings') {
      if(req.method()==='PUT') {
        writes.push(req.postDataJSON());
        if(fail) {fail=false;await route.fulfill({status:503,json:{error:'暫時無法儲存'}});return;}
        settings.routing=writes.at(-1).routing;
      }
      await route.fulfill({json:settings});
    } else if(path==='/api/providers/models')await route.fulfill({json:{models:['fast:latest','work','backup']}});
    else if(path==='/api/settings/routing/health')await route.fulfill({json:{ready:true,engines:{semantic:'ready',laya:'ready'}}});
    else await route.fulfill({json:path==='/api/agents'?{agents:[],groups:[]}:{}});
  });
  await page.goto('/');await page.getByRole('button',{name:'設定',exact:true}).click();
  if((page.viewportSize()?.width || 1280)>700)await page.getByRole('button',{name:'模型',exact:true}).click();
  await page.getByText('進階：執行核心與對話／工作分流',{exact:true}).click();
  const panel=page.getByRole('region',{name:'對話與工作分流'});
  const base=page.getByLabel('API 網址',{exact:true});
  await base.fill('https://unsaved.example/v1');
  await panel.getByRole('combobox',{name:'聊天模型',exact:true}).selectOption(JSON.stringify(['fixture','fast:latest']));
  await panel.getByLabel('聊天模型 · 推理強度').selectOption('low');
  await panel.getByLabel('工作模型',{exact:true}).selectOption(JSON.stringify(['fixture','work']));
  await panel.getByLabel('工作模型 · 推理強度').selectOption('high');
  await panel.locator('summary').click();
  await panel.getByLabel('備援模型 1',{exact:true}).selectOption(JSON.stringify(['fixture','backup']));
  await panel.getByLabel('Laya 多語模型',{exact:true}).check();
  await panel.getByLabel('最長等待（毫秒）',{exact:true}).fill('200');
  await panel.getByRole('button',{name:'儲存分流設定',exact:true}).click();
  await expect(panel.getByRole('alert')).toHaveText('暫時無法儲存');
  await expect(panel.getByLabel('聊天模型 · 推理強度')).toHaveValue('low');
  await panel.getByRole('button',{name:'儲存分流設定',exact:true}).click();
  await expect.poll(()=>writes.length).toBe(2);
  expect(writes[1].routing).toMatchObject({chat:{model:'fast:latest',reasoning_effort:'low'},work:{model:'work',reasoning_effort:'high'},
    fallbacks:[{model:'backup'}],laya:true,classifier_timeout_ms:200,classifier_shadow:true});
  expect(writes[1].provider).toBeUndefined();
  await expect(base).toHaveValue("https://unsaved.example/v1");
});
