import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from '../apps/web/node_modules/typescript/lib/typescript.js';
function loadCatalog(file){
  const code=ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText;
  const sandbox={exports:{},require:()=>({})};
  vm.runInNewContext(code,sandbox);
  return sandbox.exports.zhTW||sandbox.exports.en;
}
const zhTW=loadCatalog('apps/web/src/locales/zh-TW.ts');
const en=loadCatalog('apps/web/src/locales/en.ts');
const catalogs={'zh-TW':zhTW,en};
const i18n={locale:'en',t:(key,params)=>{const message=catalogs[i18n.locale]?.[key]??key;return params?message.replace(/\{(\w+)\}/g,(m,name)=>Object.hasOwn(params,name)?String(params[name]):m):message},getLocale:()=>i18n.locale};
const raw=fs.readFileSync('apps/web/src/schedule.tsx','utf8');
const js=ts.transpileModule(raw,{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.React}}).outputText;
const box={exports:{},require:()=>i18n};vm.runInNewContext(js,box);const {cronFromPreset,presetFromCron,defaultCronPreset,describeCron,describeCronHuman}=box.exports;
test('fixed intervals retain exact elapsed units',()=>{
 for (const unit of ['minutes','hours','days']) {const p={...defaultCronPreset(),freq:'Interval',n:45,unit};const cron=cronFromPreset(p);const restored=presetFromCron(cron);assert.equal(restored.n,45);assert.equal(restored.unit,unit);}
});
test('unknown cron is preserved verbatim instead of silently rewritten',()=>{
 for(const cron of ['0 0 9 * * 1','0 25 * * *','99 9 * * *','0 0 */3 * *','*/45 * * * *']){const p=presetFromCron(cron);assert.equal(p.freq,'Advanced');assert.equal(cronFromPreset(p),cron);}
});
test('blank advanced expressions are never converted to a scheduled job',()=>assert.equal(cronFromPreset({...defaultCronPreset(),freq:'Advanced',cron:''}),''));
test('VNC paste delegates once to confirmed backend and rejects another source',async()=>{
 const source=fs.readFileSync('apps/web/vnc.html','utf8').match(/<script type="module">([\s\S]*?)<\/script>/)[1].replace(/import RFB[^;]+;/,'');
 const handlers={},sent=[];class RFB{addEventListener(){} focus(){} sendKey(){}}
 const window={location:{pathname:'/vnc.html',protocol:'http:',host:'localhost',origin:'http://localhost',hash:''},parent:{postMessage:x=>sent.push(x)},addEventListener:(n,f)=>handlers[n]=f};
 vm.runInNewContext(source,{window,document:{location:{href:'http://localhost/vnc.html?view_only=false'},getElementById:()=>({}),querySelector:()=>null},navigator:{clipboard:{readText:async()=>'中文\nhello'}},RFB,setTimeout(){},clearTimeout(){}});
 await handlers.keydown({ctrlKey:true,code:'KeyV',preventDefault(){},stopImmediatePropagation(){}});
 assert.equal(sent.filter(x=>x.type==='lazyboy-paste-text').length,1);
 assert.equal(sent.at(-1).text,'中文\nhello');
 handlers.message({origin:'http://localhost',source:{},data:{type:'lazyboy-host-clipboard',text:'bad'}});assert.equal(sent.at(-1).text,'中文\nhello');
});

test('saved login rejects HTTP, lookalike hosts, and missing host before touching fields',()=>{
 const py=fs.readFileSync('crates/control/src/cdp.py','utf8');const expression=py.match(/FILL_LOGIN_JS = r"""([\s\S]*?)"""/)[1];
 for(const [protocol,hostname,expectedHost] of [['https:','evil.example','bank.example'],['http:','bank.example','bank.example'],['https:','bank.example.evil','bank.example'],['https:','bank.example','']]) {
  const evaluate=vm.runInNewContext(`(${expression})`,{location:{protocol,hostname},document:{querySelectorAll(){throw Error('must not touch fields')}}});
  assert.equal(evaluate({username:'u',password:'secret',expectedHost}).ok,false);
 }
});

const mdJs=ts.transpileModule(fs.readFileSync('apps/web/src/markdown.tsx','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX}}).outputText;
const mdBox={exports:{},require:(name)=>{
 if(name==='react')return{useCallback:fn=>fn,useRef:()=>({current:null}),useState:()=>[false,()=>{}],memo:fn=>fn};
 if(name==='react/jsx-runtime')return{jsx:()=>null,jsxs:()=>null,Fragment:'Fragment'};
 if(name==='react-markdown'||name==='remark-gfm'||name==='remark-breaks')return{default:()=>null};
 if(name==='./i18n')return{t:key=>key};
 if(name.endsWith('.css'))return{};
 throw new Error('unexpected import '+name);
}};
vm.runInNewContext(mdJs,mdBox);
const {sanitizeMarkdownUrl}=mdBox.exports;
const audioJs=ts.transpileModule(fs.readFileSync('apps/web/src/call-audio.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText;
const audioBox={exports:{},require:()=>{throw new Error('unexpected import')}};
vm.runInNewContext(audioJs,audioBox);
const {floatToPcm16,pcm16ToFloat,resample,VOICE_SAMPLE_RATE,CAPTURE_CHUNK_SAMPLES}=audioBox.exports;
function fakePlaybackContext(rendered){
  return {
    sampleRate:VOICE_SAMPLE_RATE,currentTime:0,
    createBuffer:(_channels,length,rate)=>{const data=new Float32Array(length);rendered.push(data);return {length,duration:length/rate,getChannelData:()=>data}},
    createBufferSource:()=>({buffer:null,connect(){},start(){},stop(){}}),
  };
}
test('pcm16 roundtrip keeps amplitude sign and resample identity at 24 kHz',()=>{
  const source=new Float32Array([0,0.5,-0.5,1,-1]);
  const bytes=floatToPcm16(source);
  const back=pcm16ToFloat(bytes.buffer);
  assert.equal(back.length,source.length);
  assert.ok(back[1]>0.49&&back[1]<0.51);
  assert.ok(back[2]< -0.49&&back[2]> -0.51);
  const same=resample(source,VOICE_SAMPLE_RATE,VOICE_SAMPLE_RATE);
  assert.equal(same,source);
  const down=resample(new Float32Array([0,1,0,1]),48000,24000);
  assert.equal(down.length,2);
});
test('markdown links only keep http(s), mailto, tel, and in-page hashes',()=>{
 assert.equal(sanitizeMarkdownUrl('https://example.com/docs'),'https://example.com/docs');
 assert.equal(sanitizeMarkdownUrl('mailto:hi@example.com'),'mailto:hi@example.com');
 assert.equal(sanitizeMarkdownUrl('#section'),'#section');
 assert.equal(sanitizeMarkdownUrl('javascript:alert(1)'),undefined);
 assert.equal(sanitizeMarkdownUrl('data:text/html,<script>alert(1)</script>'),undefined);
 assert.equal(sanitizeMarkdownUrl('/relative'),undefined);
});

test('zh-TW and en catalogs share keys and placeholders',()=>{
  const zhKeys=Object.keys(zhTW).sort();
  const enKeys=Object.keys(en).sort();
  assert.deepEqual(enKeys,zhKeys);
  for(const key of zhKeys){
    const slots=s=>[...String(s).matchAll(/\{(\w+)\}/g)].map(m=>m[1]).sort();
    assert.deepEqual(slots(en[key]),slots(zhTW[key]),key);
    assert.notEqual(String(en[key]).trim(),'');
    assert.notEqual(String(zhTW[key]).trim(),'');
  }
});

test('describeCron and stored Chinese human strings follow the UI locale',()=>{
  i18n.locale='en';
  assert.equal(describeCron('0 9 * * *'),'Daily at 9:00 AM');
  assert.equal(describeCron('0 9 * * 1-5'),'Weekdays at 9:00 AM');
  assert.equal(describeCronHuman('每天 09:00'),'Daily at 9:00 AM');
  assert.equal(describeCronHuman('每隔 45 分鐘（固定間隔）'),'Every 45 minutes (elapsed)');
  i18n.locale='zh-TW';
  assert.equal(describeCron('0 9 * * *'),'每天 09:00');
  assert.equal(describeCronHuman('Daily at 9:00 AM'),'每天 09:00');
});

 test('hanging up before microphone permission resolves releases the late stream',async()=>{
   let grant; let stopped=0;
   const context={exports:{},navigator:{mediaDevices:{getUserMedia:()=>new Promise(resolve=>grant=resolve)}}};
   vm.runInNewContext(audioJs,context);
   const audio=new context.exports.CallAudio({onCapture(){},onError(){}});
   const starting=audio.start();
   audio.stop();
   grant({getTracks:()=>[{stop(){stopped++;}}]});
   await starting;
   assert.equal(stopped,1);
 });

test('odd-sized audio frames stay sample-aligned instead of turning into noise',()=>{
  const rendered=[];
  const audio=new audioBox.exports.CallAudio({onCapture(){},onError(){}});
  audio.context=fakePlaybackContext(rendered);
  const source=new Int16Array([1000,-1000,2000,-2000,3000,-3000,4000,-4000]);
  const bytes=new Uint8Array(source.buffer);
  let offset=0;
  for(const size of [3,5,7,1]){audio.play(bytes.slice(offset,offset+size).buffer);offset+=size}
  const played=rendered.flatMap(chunk=>[...chunk]);
  const expected=[...pcm16ToFloat(bytes.slice().buffer)];
  assert.equal(played.length,expected.length);
  expected.forEach((value,index)=>assert.ok(Math.abs(played[index]-value)<1e-6,`sample ${index}`));
});

test('microphone frames are batched instead of sent one websocket message per block',()=>{
  const sent=[];
  const audio=new audioBox.exports.CallAudio({onCapture:pcm=>sent.push(pcm.length),onError(){}});
  const block=new Float32Array(64);
  const blocksPerChunk=CAPTURE_CHUNK_SAMPLES/block.length;
  for(let i=0;i<blocksPerChunk-1;i++)audio.queueCapture(block);
  assert.deepEqual(sent,[]);
  audio.queueCapture(block);
  assert.deepEqual(sent,[CAPTURE_CHUNK_SAMPLES*2]);
});
