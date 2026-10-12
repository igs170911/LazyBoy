import {test,expect} from '@playwright/test';
test.use({serviceWorkers:'block'});
test('recording previews and downloads without vision, then attaches to the draft without sending',async({page},info)=>{
  const id='c3275a4e-a3d6-407d-af3c-8b3943ebef8e';let uploaded:any,submitted='';
  const activity={state:'idle',running:false,queued:false,active_task_ids:[],observed_at_ms:Date.now()};
  await page.addInitScript(()=>{
    localStorage.setItem('lazyboy.locale','zh-Hant');(window as any).EventSource=class{close(){}};
    Object.defineProperty(navigator.mediaDevices,'getDisplayMedia',{value:async()=>{
      const canvas=document.createElement('canvas');canvas.width=640;canvas.height=360;const ctx=canvas.getContext('2d')!;ctx.fillStyle='#173f35';ctx.fillRect(0,0,640,360);ctx.fillStyle='white';ctx.font='24px sans-serif';ctx.fillText('Step 1: Open report',40,100);
      const stream=canvas.captureStream(10);(window as any).__recordingStream=stream;(window as any).__recordingCanvas=canvas;
      const clock=setInterval(()=>{ctx.fillStyle='#173f35';ctx.fillRect(0,320,640,40);ctx.fillStyle='white';ctx.fillText(String(Date.now()),10,350);},100);
      for(const track of stream.getTracks()){const stop=track.stop.bind(track);track.stop=()=>{clearInterval(clock);stop();};}
      return stream;
    }});
  });
  await page.route('**/api/**',async route=>{
    const path=new URL(route.request().url()).pathname;let data:any={};const agent={id,name:'教學助手',activity};
    if(path==='/api/auth/status')data={authenticated:true};
    else if(path==='/api/agents')data={agents:[agent],groups:[]};
    else if(path===`/api/agents/${id}`)data={...agent,transcript:submitted?[{role:'user',content:submitted}]:[]};
    else if(path.endsWith('/activity'))data=activity;
    else if(path.endsWith('/files'))data={files:[]};
    else if(path.endsWith('/computer'))data={ready:false};
    else if(path==='/api/media')data={capabilities:{images:false,pdf:false,text:true}};
    else if(path.endsWith('/lessons')){uploaded=route.request().postDataJSON();data={id:uploaded.id,video_url:`/api/agents/${id}/lessons/${uploaded.id}/video`};}
    else if(path.endsWith('/messages')){submitted=route.request().postDataJSON().text;data={queued:'recording-chat'};}
    await route.fulfill({json:data});
  });
  await page.goto('/');if(info.project.name==='mobile')await page.locator('.bot-row-main').first().click();
  await page.locator('form.composer textarea').fill('請幫我看這段錄影');
  await page.getByRole('button',{name:'加入檔案或錄製示範'}).click();
  await page.getByRole('button',{name:'錄製示範',exact:true}).click();
  await page.getByLabel('錄影名稱（選填）').fill('每週報表核對');
  await page.getByRole('button',{name:'開始螢幕錄影'}).click();
  await expect(page.locator('.recording-live')).toContainText('錄影中');
  await page.evaluate(()=>{const canvas=(window as any).__recordingCanvas as HTMLCanvasElement;const ctx=canvas.getContext('2d')!;ctx.fillStyle='#1f1f62';ctx.fillRect(0,0,640,360);ctx.fillStyle='white';ctx.fillText('Step 2: Check totals',40,100);});
  await page.getByRole('button',{name:'標記畫面'}).click();
  await expect(page.locator('.recording-live')).toContainText('2 秒');
  await page.getByRole('button',{name:'停止錄影',exact:true}).click();
  await expect(page.locator('.lesson-preview')).toBeVisible();
  expect(await page.evaluate(()=>(window as any).__recordingStream.getTracks().every((t:MediaStreamTrack)=>t.readyState==='ended'))).toBe(true);
  await page.getByLabel('補充說明（選填）').fill('核對所有列的合計，差異先列出原因。');
  const download=page.waitForEvent('download');await page.getByRole('link',{name:'下載錄影'}).click();const downloaded=await download;await downloaded.saveAs(info.outputPath('demonstration.webm'));
  await expect.poll(()=>page.locator('.lesson-preview').evaluate((v:HTMLVideoElement)=>v.readyState)).toBeGreaterThanOrEqual(2);
  await page.locator('.lesson-preview').evaluate((v:HTMLVideoElement)=>v.play());
  await expect.poll(()=>page.locator('.lesson-preview').evaluate((v:HTMLVideoElement)=>v.currentTime>0||v.ended)).toBe(true);
  await page.locator('.recording-lesson input[type=file]').setInputFiles(info.outputPath('demonstration.webm'));
  await expect(page.getByRole('button',{name:'加入對話'})).toBeEnabled();
  await expect.poll(()=>page.locator('.lesson-preview').evaluate((v:HTMLVideoElement)=>v.readyState)).toBeGreaterThanOrEqual(2);
  await expect(page.locator('.lesson-preview')).toHaveAttribute('poster',/^data:image\/jpeg;base64,/);
  await expect.poll(()=>page.locator('.lesson-preview').evaluate((v:HTMLVideoElement)=>Number.isFinite(v.duration)&&v.duration>0&&!v.seeking)).toBe(true);
  await page.screenshot({path:info.outputPath('recording-preview.png')});
  await page.getByRole('button',{name:'加入對話'}).click();
  await expect(page.getByRole('dialog',{name:'錄影'})).toHaveCount(0);
  expect(Buffer.from(uploaded.video,'base64').subarray(0,4)).toEqual(Buffer.from([0x1a,0x45,0xdf,0xa3]));
  expect(uploaded.frames.length).toBeGreaterThanOrEqual(2);
  expect(uploaded.frames.every((f:any)=>f.data_url.startsWith('data:image/jpeg;base64,')&&f.at_ms<=uploaded.duration_ms)).toBe(true);
  expect(submitted).toBe('');
  await expect(page.locator('form.composer textarea')).toHaveValue(/請幫我看這段錄影[\s\S]*每週報表核對[\s\S]*核對所有列/);
  await page.locator('form.composer').getByRole('button',{name:'送出',exact:true}).click();
  await expect.poll(()=>submitted).toContain('/lessons/');
  expect(submitted).not.toContain('manage_skill');
  await expect(page.locator('.chat-content').getByRole('link',{name:'每週報表核對'})).toBeVisible();
});
