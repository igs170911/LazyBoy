import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from '../apps/web/node_modules/typescript/lib/typescript.js';
const raw=fs.readFileSync('apps/web/src/schedule.tsx','utf8');
const js=ts.transpileModule(raw,{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.React}}).outputText;
const box={exports:{},require:()=>({t:x=>x})};vm.runInNewContext(js,box);const {cronFromPreset,presetFromCron,defaultCronPreset}=box.exports;
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
test('markdown links only keep http(s), mailto, tel, and in-page hashes',()=>{
 assert.equal(sanitizeMarkdownUrl('https://example.com/docs'),'https://example.com/docs');
 assert.equal(sanitizeMarkdownUrl('mailto:hi@example.com'),'mailto:hi@example.com');
 assert.equal(sanitizeMarkdownUrl('#section'),'#section');
 assert.equal(sanitizeMarkdownUrl('javascript:alert(1)'),undefined);
 assert.equal(sanitizeMarkdownUrl('data:text/html,<script>alert(1)</script>'),undefined);
 assert.equal(sanitizeMarkdownUrl('/relative'),undefined);
});
