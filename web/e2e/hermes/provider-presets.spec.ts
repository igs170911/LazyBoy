import { test, expect } from '@playwright/test';
import { DEFAULT_ROUTING } from '../../src/routing-settings';
import { mergeProviderModels } from '../../src/model-settings';

test('discovered models default to the preset protocol', () => {
  expect(mergeProviderModels(['grok'], [], 'xai', 'responses')).toEqual([{id:'grok',protocol:'responses'}]);
});

test('a Hermes preset only needs an API key', async ({page}) => {
  const writes: any[] = [], listed: any[] = [];
  const settings: any = {engine:'hermes',active:'mine',model:'local',protocol:'chat_completions',config_path:'/x',
    providers:[{id:'mine',label:'Mine',base_url:'http://127.0.0.1:9/v1',has_key:false,models:[{id:'local'}]}],routing:{...DEFAULT_ROUTING}};
  const presets = [{id:'xai',label:'xAI',description:'xAI Grok (Direct API)',base_url:'https://api.x.ai/v1',protocol:'responses',
    signup_url:'https://console.x.ai/',models:['grok-5'],local:false}];
  await page.addInitScript(() => {localStorage.setItem('lazyboy.locale','zh-Hant');(window as any).EventSource=class{close(){}};});
  await page.route('**/api/**', async route => {
    const req = route.request(), path = new URL(req.url()).pathname;
    if (path === '/api/auth/status') await route.fulfill({json:{authenticated:true}});
    else if (path === '/api/providers/presets') await route.fulfill({json:{presets}});
    else if (path === '/api/providers/models') { listed.push(req.postDataJSON()); await route.fulfill({json:{models:['grok-5','grok-5-mini']}}); }
    else if (path === '/api/settings') {
      if (req.method() === 'PUT') writes.push(req.postDataJSON());
      await route.fulfill({json:settings});
    } else await route.fulfill({json:path === '/api/agents' ? {agents:[],groups:[]} : {}});
  });
  await page.goto('/');
  await page.getByRole('button',{name:'設定',exact:true}).click();
  if ((page.viewportSize()?.width || 1280) > 700) await page.getByRole('button',{name:'模型',exact:true}).click();
  await page.getByLabel('供應商',{exact:true}).selectOption('xai');
  // The preset's URL is filled in for the user; it lives in the advanced section.
  await page.getByText('進階設定',{exact:true}).first().click();
  await expect(page.getByText('https://api.x.ai/v1',{exact:true})).toBeVisible();
  await expect(page.getByRole('link',{name:'取得 API 金鑰 ↗'})).toHaveAttribute('href','https://console.x.ai/');
  await page.getByLabel('API 金鑰',{exact:true}).fill('xai-secret');
  await page.getByLabel('API 金鑰',{exact:true}).blur();
  await expect.poll(() => listed.filter(body => body.provider === 'xai')).toEqual([{provider:'xai',base_url:'https://api.x.ai/v1',api_key:'xai-secret'}]);
  await page.getByRole('button',{name:'儲存',exact:true}).click();
  await expect.poll(() => writes.length).toBe(1);
  expect(writes[0]).toMatchObject({active:'xai',model:'grok-5',provider:{id:'xai',label:'xAI',base_url:'https://api.x.ai/v1',api_key:'xai-secret',
    models:[{id:'grok-5',protocol:'responses'},{id:'grok-5-mini',protocol:'responses'}]}});
});
