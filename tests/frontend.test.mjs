import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from '../apps/web/node_modules/typescript/lib/typescript.js';
function loadCatalog(file){
  const source=fs.readFileSync(file,'utf8');
  // transpileModule prints output even for a catalog that does not parse, so a
  // missing comma silently loads a shorter catalog and every key check below
  // still passes. Ask the parser, which actually keeps its errors.
  const parsed=ts.createSourceFile(file,source,ts.ScriptTarget.ESNext,true,ts.ScriptKind.TS);
  assert.deepEqual((parsed.parseDiagnostics||[]).map(d=>ts.flattenDiagnosticMessageText(d.messageText,' ')),[],`${file} must parse`);
  const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText;
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

function mobileViewer(){
 const source=fs.readFileSync('apps/web/vnc.html','utf8').match(/<script type="module">([\s\S]*?)<\/script>/)[1].replace(/import RFB[^;]+;/,'');
 const handlers={},options={},sent=[];
 const keyboard={value:'',focused:false,focus(){this.focused=true},blur(){this.focused=false},setSelectionRange(){}};
 const pointerEvents=[];
 const canvas={style:{},closest:selector=>selector==='#screen'?{}:null,getBoundingClientRect:()=>({left:0,top:0,width:300,height:200})};
 const elements={};
 const element=id=>elements[id]||(elements[id]={style:{},attributes:{},setAttribute(k,v){this.attributes[k]=v},querySelectorAll(){return []}});
 let rfb;
 class RFB{constructor(){rfb=this}addEventListener(){}focus(){keyboard.focused=false}
  _handleMouseMove(x,y){pointerEvents.push({type:'move',x,y})}
  _handleMouseButton(x,y,mask){pointerEvents.push({type:'button',x,y,mask})}
  _handleWheel(event){pointerEvents.push({type:'wheel',dx:event.deltaX,dy:event.deltaY})}
 }
 const body={className:'',classList:{tokens:new Set(),toggle(name,force){if(force===undefined)force=!this.tokens.has(name);if(force)this.tokens.add(name);else this.tokens.delete(name);body.className=[...this.tokens].join(' ')}}};
 const window={location:{pathname:'/vnc.html',protocol:'http:',host:'localhost',origin:'http://localhost',hash:''},parent:{postMessage:x=>sent.push(x)},addEventListener:(n,f,o)=>{handlers[n]=f;options[n]=o},matchMedia:()=>({matches:false,addEventListener(){}})};
 vm.runInNewContext(source,{window,document:{body,location:{href:'http://localhost/vnc.html?view_only=false'},getElementById:id=>id==='mobile-keyboard'?keyboard:element(id),querySelector:selector=>selector==='#screen canvas'?canvas:null},navigator:{},RFB,setTimeout(){},clearTimeout(){}});
 return {handlers,options,sent,keyboard,canvas,rfb,window,pointerEvents,elements,body};
 return {handlers,options,sent,keyboard,canvas,rfb,window,pointerEvents,elements};
}
test('direct mobile taps position without unexpectedly opening the keyboard',()=>{
 const {handlers:h,options,keyboard,canvas,rfb}=mobileViewer();
 for(const name of ['touchstart','touchmove','touchend'])assert.equal(options[name].capture,true);
 const point={clientX:20,clientY:30};
 h.touchstart({target:canvas,touches:[point]});h.touchend({touches:[]});
 assert.equal(keyboard.focused,false);assert.equal(rfb.focusOnClick,false);
 keyboard.blur();h.touchstart({target:canvas,touches:[point]});h.touchmove({touches:[{clientX:50,clientY:30}]});h.touchend({touches:[]});assert.equal(keyboard.focused,false);
 h.touchstart({target:canvas,touches:[point,point]});h.touchend({touches:[]});assert.equal(keyboard.focused,false);
 rfb.viewOnly=true;h.touchstart({target:canvas,touches:[point]});h.touchend({touches:[]});assert.equal(keyboard.focused,false);
});
test('mobile IME sends committed Chinese once and supports input-only backspace and Enter',()=>{
 const {handlers:h,keyboard,sent}=mobileViewer();
 sent.length=0;
 h.compositionstart({target:keyboard});keyboard.value+='中文';
 h.input({target:keyboard,isComposing:true});assert.equal(sent.length,0);
 h.compositionend({target:keyboard});h.input({target:keyboard,isComposing:false});
 assert.equal(sent.length,1);assert.equal(sent[0].text,'中文');
 keyboard.value='';h.input({target:keyboard,inputType:'deleteContentBackward'});
 let prevented=false;h.beforeinput({target:keyboard,inputType:'insertLineBreak',preventDefault(){prevented=true}});
 assert.equal(prevented,true);
 assert.deepEqual(sent.map(x=>x.text||x.key),['中文','BackSpace','Return']);
});
test('mobile keyboard is cleared and blurred when control is released; foreign messages cannot change control',()=>{
 const {handlers:h,keyboard,sent,window}=mobileViewer();
 keyboard.focus();keyboard.value+='secret';
 h.message({origin:window.location.origin,source:{},data:{type:'lazyboy-view-only',viewOnly:true}});
 assert.equal(keyboard.focused,true);
 h.message({origin:window.location.origin,source:window.parent,data:{type:'lazyboy-view-only',viewOnly:true}});
 assert.equal(keyboard.focused,false);assert.equal(keyboard.value,'\u200b');
 sent.length=0;keyboard.value+='blocked';h.input({target:keyboard});assert.equal(sent.length,0);
});

function pointerButton(viewer,action){
 viewer.handlers.click({target:{closest:()=>({dataset:{action}})}});
}
function pointerTouch(viewer,name,points){
 let prevented=false,stopped=false;
 viewer.handlers[name]({target:{closest:selector=>selector==='#trackpad'?{}:null},touches:points.map(([clientX,clientY])=>({clientX,clientY})),preventDefault(){prevented=true},stopImmediatePropagation(){stopped=true}});
 return prevented&&stopped;
}
test('trackpad moves relative to cursor, clamps edges and taps without opening keyboard',()=>{
 const v=mobileViewer();pointerButton(v,'mode');
 assert.equal(pointerTouch(v,'touchstart',[[30,30]]),true);
 pointerTouch(v,'touchmove',[[60,40]]);pointerTouch(v,'touchend',[]);
 assert.equal(v.pointerEvents.length,1);assert.equal(v.pointerEvents[0].x,180);assert.ok(Math.abs(v.pointerEvents[0].y-110)<.001);
 assert.equal(v.keyboard.focused,false);
 pointerTouch(v,'touchstart',[[10,10]]);pointerTouch(v,'touchend',[]);
 assert.deepEqual(v.pointerEvents.slice(-2).map(e=>e.mask),[1,0]);
 pointerTouch(v,'touchstart',[[10,10]]);pointerTouch(v,'touchmove',[[1000,-1000]]);pointerTouch(v,'touchend',[]);
 assert.equal(v.pointerEvents.at(-1).x,299);assert.equal(v.pointerEvents.at(-1).y,0);
 pointerButton(v,'keyboard');assert.equal(v.keyboard.focused,true);
});
test('two-finger trackpad scroll and right tap do not also left click',()=>{
 const v=mobileViewer();pointerButton(v,'mode');
 pointerTouch(v,'touchstart',[[20,20],[50,20]]);pointerTouch(v,'touchmove',[[20,80],[50,80]]);pointerTouch(v,'touchend',[]);
 assert.deepEqual(v.pointerEvents,[{type:'wheel',dx:-0,dy:-60}]);
 pointerTouch(v,'touchstart',[[20,20],[50,20]]);pointerTouch(v,'touchend',[[20,20]]);pointerTouch(v,'touchend',[]);
 assert.deepEqual(v.pointerEvents.slice(-2).map(e=>e.mask),[4,0]);
});
test('drag is released on cancellation, mode switch, blur and control handoff',()=>{
 for(const release of ['cancel','mode','blur','handoff']){
  const v=mobileViewer();pointerButton(v,'drag');
  assert.equal(v.pointerEvents.at(-1).mask,1);
  if(release==='cancel')v.handlers.touchcancel();
  if(release==='mode')pointerButton(v,'mode');
  if(release==='blur')v.handlers.blur();
  if(release==='handoff')v.handlers.message({origin:v.window.location.origin,source:v.window.parent,data:{type:'lazyboy-view-only',viewOnly:true}});
  assert.equal(v.pointerEvents.at(-1).mask,0,release);
  assert.equal(v.elements['pointer-drag'].attributes['aria-pressed'],'false');
 }
});
test('view-only pointer controls never send mouse input and trackpad asks for control',()=>{
 const v=mobileViewer();pointerButton(v,'mode');v.rfb.viewOnly=true;
 for(const action of ['left','right','drag','up','down'])pointerButton(v,action);
 pointerTouch(v,'touchstart',[[20,20]]);pointerTouch(v,'touchmove',[[50,50]]);pointerTouch(v,'touchend',[]);
 assert.equal(v.pointerEvents.length,0);
 assert.equal(v.sent.at(-1).type,'lazyboy-request-control');
});
test('trackpad mode swallows screen taps so fingers on the picture do not click',()=>{
 const v=mobileViewer();pointerButton(v,'mode');
 let prevented=false,stopped=false;
 v.handlers.touchstart({target:v.canvas,touches:[{clientX:20,clientY:30}],preventDefault(){prevented=true},stopImmediatePropagation(){stopped=true}});
 assert.equal(prevented&&stopped,true);
 assert.equal(v.pointerEvents.length,0);
 assert.equal(v.keyboard.focused,false);
});
test('coarse pointers start in trackpad mode with a body class for independent controls',()=>{
 const source=fs.readFileSync('apps/web/vnc.html','utf8').match(/<script type="module">([\s\S]*?)<\/script>/)[1].replace(/import RFB[^;]+;/,'');
 const body={className:'',classList:{tokens:new Set(),toggle(name,force){if(force)this.tokens.add(name);else this.tokens.delete(name);body.className=[...this.tokens].join(' ')}}};
 const window={location:{pathname:'/vnc.html',protocol:'http:',host:'localhost',origin:'http://localhost',hash:''},parent:{postMessage(){}},addEventListener(){},matchMedia:()=>({matches:true,addEventListener(){}})};
 const elements={}; const element=id=>elements[id]||(elements[id]={style:{},attributes:{},hidden:true,setAttribute(k,v){this.attributes[k]=v},querySelectorAll(){return []}});
 class RFB{constructor(){} addEventListener(){} focus(){}}
 vm.runInNewContext(source,{window,document:{body,location:{href:'http://localhost/vnc.html?view_only=false'},getElementById:id=>element(id),querySelector:()=>null},navigator:{},RFB,setTimeout(){},clearTimeout(){}});
 assert.equal(body.className,'touch-ui');
 assert.equal(elements.trackpad.hidden,false);
});
test('mobile shortcut row sends modifiers and hides with the keyboard',()=>{
 const v=mobileViewer();
 pointerButton(v,'keyboard');
 assert.equal(v.keyboard.focused,true);
 assert.equal(v.elements['keyboard-shortcuts'].hidden,false);
 v.handlers.click({target:{closest:()=>({dataset:{action:'modifier',key:'ctrl'}})}});
 v.handlers.click({target:{closest:()=>({dataset:{action:'key',key:'c'}})}});
 assert.equal(v.sent.at(-1).type,'lazyboy-mobile-key');
 assert.equal(v.sent.at(-1).key,'ctrl+c');
 v.handlers.click({target:{closest:()=>({dataset:{action:'hide-keyboard'}})}});
 assert.equal(v.keyboard.focused,false);
 assert.equal(v.elements['keyboard-shortcuts'].hidden,true);
});
test('status sits outside the remote pixels; screenshots stay opt-in',()=>{
  const app=fs.readFileSync('apps/web/src/App.tsx','utf8');
  assert.doesNotMatch(app,/RunStatus/);
 assert.match(app,/<RunProbe runId=\{computer\.busyRunId\|\|computer\.waitingRunId\}><Avatar/);
  assert.match(app,/--visible-height/);
 assert.match(app,/<div className=\{`composer-dock \$\{statusMembers\.length\?"has-status":""\}`\}>\s*\{error&&<div className="error-banner"/);
 const css=fs.readFileSync('apps/web/src/computer.css','utf8');
 assert.doesNotMatch(css,/\.run-status/);
  const chat=fs.readFileSync('apps/web/src/chat.css','utf8');
 assert.match(chat,/\.composer-dock\{position:relative/);
 assert.doesNotMatch(chat,/\.composer-dock\{position:absolute/);
 assert.match(chat,/\.messages\{min-width:0;padding-bottom:24px/);
 const tools=fs.readFileSync('crates/api/src/tools.rs','utf8');
 assert.match(tools,/screenshots are opt-in with observe:true/);
 assert.match(tools,/if args\.get\("observe"\)\.and_then\(Value::as_bool\) != Some\(true\)/);
 const runs=fs.readFileSync('crates/api/src/runs.rs','utf8');
 assert.match(runs,/Never silently stop/);
 assert.match(runs,/let goal_mode = goal_mode \|\| !chat_only/);
});

const monitorJs=ts.transpileModule(fs.readFileSync('apps/web/src/run-monitor.tsx','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX}}).outputText;
const monitorBox={exports:{},require:name=>{
 if(name==='react')return{useCallback:fn=>fn,useEffect:()=>{},useRef:()=>({current:null}),useState:()=>[null,()=>{}]};
 if(name==='react/jsx-runtime')return{jsx:()=>null,jsxs:()=>null,Fragment:'Fragment'};
 if(name==='./api')return{api:async()=>({activity:[]})};
 if(name==='./i18n')return{t:i18n.t,getLocale:()=>i18n.locale};
 throw new Error('unexpected import '+name);
}};
vm.runInNewContext(monitorJs,monitorBox);
const {formatElapsed,shortDuration,errorActions,errorTitle,trailText}=monitorBox.exports;
const FAILURE_CODES=['interrupted','tool_timeout','model_key','model_quota','model_unknown','model_timeout','network','computer_gone','lease_lost','unknown'];

test('run timings stay on one glanceable line',()=>{
 assert.equal(formatElapsed(0),'0:00');
 assert.equal(formatElapsed(9_500),'0:09');
 assert.equal(formatElapsed(65_000),'1:05');
 assert.equal(formatElapsed(3_725_000),'1:02:05');
 assert.equal(formatElapsed(-5),'0:00');
 assert.equal(formatElapsed(NaN),'0:00');
 assert.equal(shortDuration(340),'340ms');
 assert.equal(shortDuration(6_400),'6.4s');
 assert.equal(shortDuration(65_000),'1:05');
 assert.equal(shortDuration(null),'');
 assert.equal(shortDuration(-1),'');
});

test('every failure code offers at least one action, in the right order',()=>{
 const buttons=code=>errorActions(code).join(',');
 assert.equal(buttons('model_key'),'settings,retry');
 assert.equal(buttons('model_unknown'),'settings,retry');
 assert.equal(buttons('model_quota'),'retry,settings');
 assert.equal(buttons('model_timeout'),'retry,settings');
 assert.equal(buttons('network'),'retry,settings');
 assert.equal(buttons('computer_gone'),'screen,retry');
 assert.equal(buttons('tool_timeout'),'screen,retry');
 assert.equal(buttons('interrupted'),'screen,retry');
 assert.equal(buttons('lease_lost'),'retry');
 for(const code of [...FAILURE_CODES,'made_up',undefined,null]){
  const actions=errorActions(code);
  assert.ok(actions.length>0,String(code));
  assert.ok(actions.every(action=>['retry','screen','settings'].includes(action)),code);
  assert.equal(new Set(actions).size,actions.length,'no duplicate buttons for '+code);
 }
});

test('failure titles resolve in both catalogs and never repeat themselves',()=>{
 for(const locale of Object.keys(catalogs)){
  i18n.locale=locale;
  const titles=FAILURE_CODES.map(code=>errorTitle(code));
  for(const [index,title] of titles.entries()){
   assert.notEqual(title,title.startsWith('errorTitle')?title:`${title}-missing`,FAILURE_CODES[index]);
   assert.ok(!title.startsWith('errorTitle'),`${locale} ${FAILURE_CODES[index]} has no copy`);
   assert.ok(title.trim().length>0);
  }
  assert.equal(new Set(titles).size,titles.length,locale);
  assert.ok(errorTitle('made_up').length>0);
 }
 i18n.locale='en';
});

test('trail lines read as sentences with the detail a stuck run needs',()=>{
 assert.equal(trailText({id:1,kind:'model',createdAt:'',turn:3,elapsedMs:6400,text:'先打開網頁'}),'turn 3, thought for 6.4s — 先打開網頁');
 assert.match(trailText({id:2,kind:'tool',createdAt:'',step:'browser: click #12',status:'ok',elapsedMs:400}),/browser: click #12 · ok · 400ms/);
 assert.match(trailText({id:3,kind:'tool',createdAt:'',step:'shell: npm test',status:'timed_out',snippet:'killed after 150s'}),/timed out — killed after 150s/);
 assert.match(trailText({id:4,kind:'run',createdAt:'',event:'started',task:'整理下載資料'}),/整理下載資料/);
 assert.match(trailText({id:5,kind:'run',createdAt:'',event:'completed',turns:9}),/Done in 9 turns/);
 assert.match(trailText({id:6,kind:'run',createdAt:'',event:'waiting_input',reason:'登入'}),/Waiting for you: 登入/);
 assert.match(trailText({id:10,kind:'run',createdAt:'',event:'paused',reason:'已達本輪執行上限'}),/Paused: 已達本輪執行上限/);
 assert.match(trailText({id:7,kind:'run',createdAt:'',event:'retry'}),/Re-queued/);
 assert.match(trailText({id:8,kind:'retry',createdAt:'',attempt:2,gaveUp:true,error:'429 rate limit'}),/attempt 2 failed, retrying · gave up — 429 rate limit/);
 assert.match(trailText({id:9,kind:'notice',createdAt:'',text:'這輪不需要電腦'}),/這輪不需要電腦/);
});

test('the bubble reads the run activity endpoint the API actually mounts',()=>{
 const monitor=fs.readFileSync('crates/api/src/monitor.rs','utf8');
 assert.match(monitor,/\.route\("\/api\/runs\/\{id\}\/activity", get\(activity\)\)/);
 assert.match(monitor,/\.route\("\/api\/runs\/\{id\}\/retry", post\(retry\)\)/);
 const app=fs.readFileSync('apps/web/src/App.tsx','utf8');
 assert.match(app,/chip\.kind==="error"\?/);
 assert.match(app,/errorActions\(chip\.code\)/);
 assert.match(app,/`\/api\/runs\/\$\{runId\}\/retry`,\{method:"POST",body:"\{\}"\}/);
 assert.match(app,/<RunProbe runId=\{chip\.runId\|\|null\} align="end" label=\{t\("errorDetails"\)\}/);
 assert.match(app,/<RunProbe runId=\{member\.id===computer\.botId\?computer\.busyRunId:null\}><Avatar/);
 const probe=fs.readFileSync('apps/web/src/run-monitor.tsx','utf8');
 assert.match(probe,/`\/api\/runs\/\$\{runId\}\/activity\$\{after\}`/);
 assert.doesNotMatch(probe,/export function RunStatus/);
 assert.match(fs.readFileSync('apps/web/src/main.tsx','utf8'),/import "\.\/monitor\.css";/);
});

const liveJs=ts.transpileModule(fs.readFileSync('apps/web/src/live.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText;
const liveBox={exports:{}};vm.runInNewContext(liveJs,liveBox);
const {sessionEventsUrl,subscribeToSession,SESSION_EVENT_TYPES,createCoalescer}=liveBox.exports;

test('the browser listens for every event kind the api can append',()=>{
  const rust=['crates/api/src/runs.rs','crates/api/src/sessions.rs','crates/api/src/schedules.rs','crates/api/src/voice_call.rs'].map(file=>fs.readFileSync(file,'utf8')).join('\n');
  const emitted=[...rust.matchAll(/["']((?:message|run|session)\.[a-z_]+)["']/g)].map(match=>match[1]);
  assert.ok(emitted.length>=4,'the api should emit session events');
  for(const kind of new Set(emitted))assert.ok(SESSION_EVENT_TYPES.includes(kind),`${kind} is emitted but never listened for`);
});

test('the live feed reads its own session, drops replays, and survives a bad frame',()=>{
  const listeners={};let closed=0,url='';
  const source={addEventListener:(type,listener)=>{listeners[type]=listener},close:()=>{closed+=1}};
  const seen=[],statuses=[];
  const feed=subscribeToSession('s/1',event=>seen.push(event),{source:candidate=>{url=candidate;return source},onStatus:connected=>statuses.push(connected)});
  assert.equal(url,'/api/sessions/s%2F1/events');
  assert.ok(listeners['message.created']&&listeners['run.completed']&&listeners['session.cleared'],'every kind gets a listener');
  listeners['message.created']({lastEventId:'5',data:'{"seq":5,"role":"assistant"}'});
  // The event object was built inside a vm realm, so compare fields, not prototypes.
  assert.equal(seen.length,1);
  assert.equal(seen[0].kind,'message.created');assert.equal(seen[0].id,5);
  assert.equal(seen[0].payload.seq,5);assert.equal(seen[0].payload.role,'assistant');
  listeners['message.created']({lastEventId:'5',data:'{}'});
  listeners['message.created']({lastEventId:'4',data:'{}'});
  assert.equal(seen.length,1,'a reconnect replay must not be applied twice');
  listeners['run.started']({lastEventId:'6',data:'not json'});
  assert.equal(seen.length,2);assert.equal(seen[1].kind,'run.started');assert.equal(seen[1].id,6);
  assert.equal(Object.keys(seen[1].payload).length,0,'a malformed frame still reports the event');
  listeners['open']();listeners['error']();
  assert.deepEqual(statuses,[true,false]);
  feed.close();feed.close();
  assert.equal(closed,1,'close is idempotent');
  assert.equal(sessionEventsUrl('abc'),'/api/sessions/abc/events');
});

test('a burst of session events settles into one refresh',()=>{
  const timers=[];let runs=0;
  const schedule=(callback,ms)=>{timers.push({callback,ms});return timers.length-1};
  const dismiss=handle=>{if(timers[handle])timers[handle].cancelled=true};
  const coalescer=createCoalescer(()=>{runs+=1},120,schedule,dismiss);
  coalescer.kick();coalescer.kick();coalescer.kick();
  assert.equal(runs,0);
  assert.equal(timers.filter(entry=>!entry.cancelled).length,1,'one pending run');
  assert.equal(timers[0].ms,120);
  timers[0].callback();
  assert.equal(runs,1);
  coalescer.kick();coalescer.cancel();coalescer.cancel();
  assert.equal(timers[1].cancelled,true);
  assert.equal(runs,1,'a cancelled run never fires');
  coalescer.kick();timers[2].callback();
  assert.equal(runs,2,'the coalescer is reusable after a cancel');
});

test('chat follows the event stream instead of a fixed two second poll',()=>{
  const app=fs.readFileSync('apps/web/src/App.tsx','utf8');
  assert.match(app,/subscribeToSession\(activeSessionId,\(\)=>settle\.kick\(\)/);
  assert.match(app,/createCoalescer\(\(\)=>\{if\(!document\.hidden\)refresh\(\)\.catch\(\(\)=>\{\}\)\},EVENT_SETTLE_MS\)/);
  assert.match(app,/document\.addEventListener\("visibilitychange",resume\)/);
  assert.doesNotMatch(app,/const timer=setInterval\(\(\)=>\{refresh\(\)/,'the 2s transcript poll should be gone');
  assert.match(app,/const heartbeat=window\.setInterval\(\(\)=>\{const beat=roomsRef\.current[\s\S]*\},HEARTBEAT_MS\)/,'the heartbeat keeps its own minute cadence');
});

// The screen veil and the frame that survives it: both used to be driven by a
// timer plus "null the url", which is what made booting and handing over feel
// like a stall instead of a gesture.
const handoffJs=ts.transpileModule(fs.readFileSync('apps/web/src/handoff.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText;
const handoffBox={exports:{},require:()=>({})};vm.runInNewContext(handoffJs,handoffBox);
const {viewerPath,keepScreenUrl,handoffRemaining,nextVeil,viewOnlyFor,HANDOFF_MS,HANDOFF_MIN_MS,VEIL_FADE_MS}=handoffBox.exports;

test('the viewer path is stable, so a frame can mount before the desktop answers',()=>{
  assert.equal(viewerPath('bot 7'),'/view/bot%207/vnc.html');
  assert.equal(viewerPath('a/b'),'/view/a%2Fb/vnc.html');
  assert.equal(viewerPath('bot'),viewerPath('bot'),'no nonce, or the frame would remount every poll');
});

test('the path the frame mounts on is the byte the api hands back',()=>{
  // The viewer mounts on the client's own path while the desktop is coming up,
  // then adopts the url from the status round trip. If the two ever differ the
  // iframe remounts mid-boot, which is the black flash this whole path exists
  // to avoid, so the invariant is pinned to the server's own format string.
  const routes=fs.readFileSync('crates/api/src/routes.rs','utf8');
  const served=(routes.match(/"url": format!\("([^"]+)"\)/)||[])[1];
  assert.equal(served,'/view/{id}/vnc.html');
  const id='6f1d3a0e-2b7c-4d5e-8f90-1a2b3c4d5e6f';
  assert.equal(viewerPath(id),served.replace('{id}',id));
});

test('pixels are dropped only for a computer that is really gone',()=>{
  const url='/view/bot/vnc.html';
  for(const state of ['booting','suspended','running'])assert.equal(keepScreenUrl(url,null,state),url,state+' must keep its VNC session');
  for(const state of ['stopped','error'])assert.equal(keepScreenUrl(url,null,state),null,state+' has no desktop to keep');
  assert.equal(keepScreenUrl(null,null,'running'),null,'nothing was ever mounted');
  assert.equal(keepScreenUrl(url,'/other','booting'),'/other','a fresh url still wins');
  assert.equal(keepScreenUrl(null,url,'booting'),url,'the first url arrives while booting');
});

test('a handoff holds one visible beat, never the whole round trip',()=>{
  assert.ok(HANDOFF_MIN_MS<HANDOFF_MS,'the beat needs a floor and a ceiling');
  assert.ok(VEIL_FADE_MS<=HANDOFF_MIN_MS,'the fade has to finish inside the beat');
  assert.ok(HANDOFF_MS<=1_000,'a reply that never lands cannot strand the mascot');
  assert.equal(handoffRemaining(1_000,1_000),HANDOFF_MIN_MS,'a reply this instant still owes the beat');
  assert.equal(handoffRemaining(1_000,1_000+HANDOFF_MIN_MS),0,'served');
  assert.equal(handoffRemaining(1_000,1_000+(HANDOFF_MIN_MS+HANDOFF_MS)/2),0,'past the floor is never a wait');
  assert.equal(handoffRemaining(1_000,1_000+HANDOFF_MS),0);
  const midway=handoffRemaining(1_000,1_000+HANDOFF_MIN_MS-10);
  assert.equal(midway,10,'the remaining beat shrinks with the clock');
});

test('the veil fades in with its label and back out through the same mascot',()=>{
  const empty={label:null,leaving:false};
  assert.equal(nextVeil(null,empty),empty,'with nothing to say there is nothing to show');
  const shown=nextVeil('換手中',empty);
  assert.equal(shown.label,'換手中');assert.equal(shown.leaving,false);
  assert.equal(nextVeil('換手中',shown),shown,'a repeated status must not restart the animation');
  const leaving=nextVeil(null,shown);
  assert.equal(leaving.label,'換手中','the label is held while it fades');assert.equal(leaving.leaving,true);
  assert.equal(nextVeil(null,leaving),leaving,'it leaves exactly once');
  const replaced=nextVeil('喚醒中',shown);
  assert.equal(replaced.label,'喚醒中');assert.equal(replaced.leaving,false,'a new status replaces the label');
  const revived=nextVeil('喚醒中',leaving);
  assert.equal(revived.label,'喚醒中');assert.equal(revived.leaving,false,'and pulls it back out of the fade');
});

test('only a human moves the mouse, and the veil never holds it back',()=>{
  assert.equal(viewOnlyFor('user'),false);
  for(const holder of ['none','bot'])assert.equal(viewOnlyFor(holder),true,holder);
});

test('taking the screen flips the mouse on the click, not on the reply',()=>{
  const app=fs.readFileSync('apps/web/src/App.tsx','utf8');
  const body=app.slice(app.indexOf('async function setControl('));
  const flip=body.indexOf('controlHolder:holder');
  const gate=body.indexOf('pushViewOnly(viewOnlyFor(holder))');
  const post=body.indexOf('await api(`/api/computer/${botId}/${holder==="user"?"takeover":"release"}`');
  const readBack=body.indexOf('finally{');
  assert.ok(flip>0&&gate>0&&post>0&&readBack>0,'optimistic flip, gate, request, and read back all present');
  assert.ok(flip<post&&gate<post,'badge and input move before the server answers');
  assert.ok(readBack>post,'the truth is read back after the request');
  assert.match(body.slice(readBack),/await refresh\(\)/);
  assert.match(body,/if\(!botId\|\|controlBusyRef\.current\)return/,'a double click cannot fight itself');
  assert.match(app,/const listener=\(event:MessageEvent\)=>\{[\s\S]*?lazyboy-request-control[\s\S]*?void setControl\("user"\)/);
  assert.match(app,/if\(expectedHolderRef\.current&&status\.controlHolder===expectedHolderRef\.current\)/,'agreement, not a timer, ends the handoff');
  assert.match(app,/onClick=\{onTakeOver\}/);
  assert.match(app,/onClick=\{onRelease\}/);
  assert.doesNotMatch(app,/action\(\(\)=>api\(`\/api\/computer\/[^`]*takeover/,'takeover no longer rides the global busy path');
  assert.equal((app.match(/\/api\/computer\/\$\{[^}]*\}\/(takeover|release)/g)||[]).length,0,'every handoff goes through setControl');
  assert.equal((app.match(/holder==="user"\?"takeover":"release"/g)||[]).length,1,'one request path, one owner of it');
  assert.match(app,/void setControl\("user",active\.id\)/,'the call overlay hands over the same way');
  assert.match(app,/await setControl\("user",id\)/,'the login screen boots, then takes the mouse without a second spinner');
});

test('the waiting veil covers the desktop the way it always did',()=>{
  const app=fs.readFileSync('apps/web/src/App.tsx','utf8');
  assert.match(app,/<div className="preview">\{computerOpen\?<EmptyComputer state=\{computer\.state\}\/>:frame\}\{!computerOpen&&hud\}<\/div>/);
  assert.match(app,/<div className="overlay-desktop">\{frame\}\{hud\}<\/div>/);
  assert.match(app,/const hud=paneBot&&veil\.label\?<ComputerHud bot=\{paneBot\} label=\{veil\.label\} leaving=\{veil\.leaving\}\/>:null/);
  const css=fs.readFileSync('apps/web/src/computer.css','utf8');
  // Both parents are what make inset:0 cover the desktop rather than the page.
  assert.match(css,/\.computer-part \.preview\{position:relative/);
  assert.match(css,/\.overlay-screen \.overlay-desktop\{position:relative/);
  // These values were compared against main in a real browser: rendered pixel
  // for pixel identical, so drift here means the veil visibly changed.
  const hud=css.match(/^\.computer-hud\{([^}]*)\}/m)[1];
  for(const rule of ['position:absolute','inset:0','z-index:3','display:grid','align-content:center','justify-items:center','gap:14px','padding:10px 12px 8px','border-radius:inherit','background:radial-gradient(ellipse at center,#142722e8,#101012ed)','backdrop-filter:blur(8px)','pointer-events:none'])assert.ok(hud.includes(rule),`the veil needs ${rule}`);
  assert.match(css,/\.computer-hud\.is-leaving\{opacity:0\}/);
  assert.match(css,/\.computer-hud\{[^}]*transition:opacity \.22s ease\}/,'it fades instead of blinking off');
  // main drew the label as static mint text; the shimmer belongs to the chat
  // working label, and the veil keeps the mascot as the only moving part.
  const label=css.match(/^\.computer-hud-label\{([^}]*)\}/m)[1];
  assert.ok(!label.includes('animation'),'the veil label does not animate');
  assert.ok(label.includes('color:#c9ddd5')&&label.includes('font-size:12px')&&label.includes('text-align:center')&&label.includes('max-width:90%'));
  assert.ok(!/\.computer-hud[^{]*\{[^}]*working-shimmer/.test(css),'nothing shimmers inside the veil');
  assert.match(css,/\.computer-signal\{[^}]*width:64px[^}]*animation:monitor-breathe/);
  assert.match(css,/\.computer-signal::before\{[^}]*monitor-orbit/);
  const responsive=fs.readFileSync('apps/web/src/responsive.css','utf8');
  assert.match(responsive,/prefers-reduced-motion:reduce\)\{\.computer-signal,\.computer-signal::before,\.computer-signal-face i[^}]*animation:none/,'reduced motion still has to still the mascot');
});

test('a desktop that is still starting is dialled again fast, a dropped session calmly',()=>{
  const source=fs.readFileSync('apps/web/vnc.html','utf8').match(/<script type="module">([\s\S]*?)<\/script>/)[1].replace(/import RFB[^;]+;/,'');
  // Any element the viewer touches answers, so the disconnect path runs the
  // same way it does in a browser without a DOM to stand in.
  const element=()=>new Proxy({style:{},classList:{toggle(){}},dataset:{},hidden:false,textContent:''},{get:(node,key)=>key in node?node[key]:()=>undefined,set:(node,key,value)=>{node[key]=value;return true}});
  const instances=[],scheduled=[];
  class RFB{constructor(){this._handlers={};instances.push(this._handlers);}
    addEventListener(name,handler){(this._handlers[name]||(this._handlers[name]=[])).push(handler);} focus(){} blur(){} sendKey(){}}
  const window={location:{pathname:'/vnc.html',protocol:'http:',host:'localhost',origin:'http://localhost',hash:''},parent:{postMessage(){}},addEventListener(){}};
  vm.runInNewContext(source,{window,document:{location:{href:'http://localhost/vnc.html?view_only=false'},getElementById:()=>element(),querySelector:()=>null},navigator:{clipboard:{}},RFB,setTimeout:(fn,ms)=>{scheduled.push(ms);return scheduled.length},clearTimeout(){}});
  const fire=(name,event)=>{for(const handlers of instances)(handlers[name]||[]).forEach(handler=>handler(event||{}));};
  assert.equal(instances.length,1,'the viewer dials once on load');
  fire('disconnect',{clean:false});
  assert.equal(scheduled.at(-1),350,'a desktop that has never answered is retried eagerly');
  fire('connect');
  fire('disconnect',{clean:true});
  assert.equal(scheduled.at(-1),1500,'a session that was really there is retried calmly');
});

test('a session dialed after a handoff starts with the mouse already handed over',()=>{
  const vnc=fs.readFileSync('apps/web/vnc.html','utf8');
  // Run the viewer rather than read it. The host hands the mouse over while the
  // viewer sits between two sessions: exactly the moment a fresh RFB used to
  // fall back to its view-only default and leave a taken-over desktop deaf with
  // the veil already lifted. Only the gate message arriving is real here.
  const body=vnc.match(/<script type="module">([\s\S]*?)<\/script>/)[1].replace(/import RFB[^;]+;/,'');
  const element=()=>new Proxy({style:{},classList:{toggle(){}},dataset:{},hidden:false,textContent:''},{get:(node,key)=>key in node?node[key]:()=>undefined,set:(node,key,value)=>{node[key]=value;return true}});
  const instances=[],scheduled=[],messages=[];
  class RFB{constructor(){this._handlers={};instances.push(this);}addEventListener(name,handler){(this._handlers[name]||(this._handlers[name]=[])).push(handler);}focus(){this.focused=true;}blur(){}}
  const parent={postMessage(){}};
  const window={location:{pathname:'/vnc.html',protocol:'http:',host:'localhost',origin:'http://localhost',hash:''},parent,addEventListener(name,handler){if(name==='message')messages.push(handler);}};
  vm.runInNewContext(body,{window,document:{location:{href:'http://localhost/vnc.html'},getElementById:()=>element(),querySelector:()=>null},navigator:{clipboard:{}},RFB,setTimeout:(fn)=>{scheduled.push(fn);return scheduled.length},clearTimeout(){}});
  const fire=(name,event)=>instances.forEach(rfb=>(rfb._handlers[name]||[]).forEach(handler=>handler(event||{})));
  fire('disconnect',{clean:false});
  assert.ok(messages.length,'the viewer listens for the host gate');
  messages.forEach(handler=>handler({origin:'http://localhost',source:parent,data:{type:'lazyboy-view-only',viewOnly:false}}));
  scheduled.at(-1)();
  fire('connect');
  assert.equal(instances.length,2,'the retry dialed a new session');
  assert.equal(instances.at(-1).viewOnly,false,'a handoff made mid-reconnect still owns the new session');
  assert.equal(instances.at(-1).focused,true,'taking over lands the first keystroke');
});
