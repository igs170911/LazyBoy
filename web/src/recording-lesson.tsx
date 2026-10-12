import { useDialogFocus } from "./dialog-focus";
import {useEffect,useRef,useState} from 'react';
import {api,type AgentRow} from './api';

type Frame={at_ms:number;data_url:string};
type Clip={blob:Blob;url:string;duration_ms:number;frames:Frame[];id:string};
const LIMIT=120000,MAX_BYTES=32*1024*1024;
function uuid(){const b=crypto.getRandomValues(new Uint8Array(16));b[6]=(b[6]&15)|64;b[8]=(b[8]&63)|128;const hex=[...b].map(v=>v.toString(16).padStart(2,'0')).join('');return `${hex.slice(0,8)}-${hex.slice(8,12)}-${hex.slice(12,16)}-${hex.slice(16,20)}-${hex.slice(20)}`;}
function base64(blob:Blob){return new Promise<string>((resolve,reject)=>{const r=new FileReader();r.onload=()=>resolve(String(r.result).split(',')[1]);r.onerror=()=>reject(new Error('無法讀取錄影'));r.readAsDataURL(blob);});}
function image(video:HTMLVideoElement){
  const canvas=document.createElement('canvas');const ratio=Math.min(1,1280/video.videoWidth);canvas.width=Math.round(video.videoWidth*ratio);canvas.height=Math.round(video.videoHeight*ratio);
  if(!canvas.width||!canvas.height)throw new Error('畫面尚未載入，請重新錄影');
  canvas.getContext('2d')!.drawImage(video,0,0,canvas.width,canvas.height);return canvas.toDataURL('image/jpeg',.78);
}
function videoEvent(video:HTMLVideoElement,event:string){return new Promise<void>((resolve,reject)=>{
  const clean=()=>{clearTimeout(timer);video.removeEventListener(event,done);video.removeEventListener('error',fail);};
  const done=()=>{clean();resolve();},fail=()=>{clean();reject(new Error('無法讀取這支影片，請改用 WebM 或 MP4'));};
  const timer=setTimeout(fail,10000);video.addEventListener(event,done,{once:true});video.addEventListener('error',fail,{once:true});
});}

export function RecordingLesson({agent,onClose,onAttach}:{agent:Pick<AgentRow,'id'|'name'>;onClose:()=>void;onAttach:(text:string)=>void}){
  const [title,setTitle]=useState(''),[notes,setNotes]=useState(''),[mode,setMode]=useState<'idle'|'recording'|'processing'|'ready'|'uploading'>('idle');
  const [clip,setClip]=useState<Clip|null>(null),[elapsed,setElapsed]=useState(0),[frameCount,setFrameCount]=useState(0),[error,setError]=useState('');
  const [stage,setStage]=useState(''),[uploaded,setUploaded]=useState<{id:string;video_url:string;manifest_path?:string}|null>(null);
  const live=useRef(true),recorder=useRef<MediaRecorder|null>(null),stream=useRef<MediaStream|null>(null),captureVideo=useRef<HTMLVideoElement|null>(null),frames=useRef<Frame[]>([]),timer=useRef<ReturnType<typeof setInterval>|undefined>(undefined),started=useRef(0),sourceUrl=useRef(''),clipRef=useRef<Clip|null>(null),inFlight=useRef(false),input=useRef<HTMLInputElement>(null),root=useRef<HTMLDivElement>(null);
  clipRef.current=clip;
  useEffect(()=>{live.current=true;const before=document.activeElement as HTMLElement|null;root.current?.querySelector<HTMLInputElement>('input')?.focus();return()=>{live.current=false;if(timer.current)clearInterval(timer.current);if(recorder.current?.state==='recording')recorder.current.stop();stream.current?.getTracks().forEach(t=>t.stop());if(sourceUrl.current)URL.revokeObjectURL(sourceUrl.current);before?.focus();};},[]);
  function replaceClip(next:Clip){if(sourceUrl.current)URL.revokeObjectURL(sourceUrl.current);sourceUrl.current=next.url;setClip(next);setUploaded(null);setMode('ready');setElapsed(next.duration_ms);setFrameCount(next.frames.length);}
  function capture(){const video=captureVideo.current;if(!video||frames.current.length>=64)return;const at_ms=Math.min(LIMIT,Date.now()-started.current);frames.current.push({at_ms,data_url:image(video)});setFrameCount(frames.current.length);}
  function stop(){if(recorder.current?.state==='recording'){try{capture();}catch{}recorder.current.stop();}if(timer.current)clearInterval(timer.current);stream.current?.getTracks().forEach(t=>t.stop());}
  async function start(){
    if(inFlight.current)return;inFlight.current=true;setError('');
    try{
      // Must stay directly inside the user's click handler for screen capture permission.
      const selected=await navigator.mediaDevices.getDisplayMedia({video:{frameRate:10},audio:false});stream.current=selected;
      if(!live.current){selected.getTracks().forEach(t=>t.stop());return;}
      const video=document.createElement('video');video.muted=true;video.srcObject=selected;captureVideo.current=video;await video.play();
      if(!live.current){selected.getTracks().forEach(t=>t.stop());return;}
      frames.current=[];started.current=Date.now();capture();setElapsed(0);
      const mime=['video/webm;codecs=vp9','video/webm;codecs=vp8','video/mp4'].find(v=>MediaRecorder.isTypeSupported(v));
      const r=new MediaRecorder(selected,{...(mime?{mimeType:mime}:{}),videoBitsPerSecond:1200000});recorder.current=r;
      const chunks:Blob[]=[];let bytes=0;
      r.ondataavailable=e=>{if(e.data.size){chunks.push(e.data);bytes+=e.data.size;if(bytes>MAX_BYTES){setError('錄影已達 32 MB，請縮短後重新錄製');stop();}}};
      r.onerror=()=>{if(live.current)setError('錄影中斷，請檢查預覽或重新錄製');stop();};
      r.onstop=()=>{if(timer.current)clearInterval(timer.current);selected.getTracks().forEach(t=>t.stop());video.srcObject=null;if(!live.current)return;const blob=new Blob(chunks,{type:r.mimeType||'video/webm'});if(!blob.size||blob.size>MAX_BYTES){setMode('idle');return;}const duration_ms=Math.max(1,Date.now()-started.current);if(duration_ms>125000)setError('錄影因休眠或背景暫停超過 2 分鐘，請下載後裁短，或重新錄製');replaceClip({blob,url:URL.createObjectURL(blob),duration_ms,frames:[...frames.current],id:uuid()});};
      selected.getVideoTracks()[0].addEventListener('ended',stop,{once:true});r.start(500);setMode('recording');
      timer.current=setInterval(()=>{const at=Date.now()-started.current;setElapsed(Math.min(LIMIT,at));try{capture();}catch(e){setError(e instanceof Error?e.message:'畫面擷取失敗');stop();}if(at>=LIMIT||frames.current.length>=64)stop();},2000);
    }catch(e){stream.current?.getTracks().forEach(t=>t.stop());if(live.current){setMode(clipRef.current?'ready':'idle');setError(e instanceof DOMException&&e.name==='NotAllowedError'?'未取得螢幕分享權限，可以再試一次或上傳錄影。':e instanceof Error?e.message:'無法開始錄影');}}
    finally{inFlight.current=false;}
  }
  async function importVideo(file:File){
    if(inFlight.current)return;
    if(file.size>MAX_BYTES){setError('錄影最多 32 MB，請先縮短影片');return;}
    inFlight.current=true;setMode('processing');setError('');const url=URL.createObjectURL(file),video=document.createElement('video');video.muted=true;video.preload='auto';
    try{
      const loaded=videoEvent(video,'loadedmetadata');video.src=url;await loaded;
      if(!Number.isFinite(video.duration)){const end=videoEvent(video,'durationchange');video.currentTime=1e10;await end;}
      if(!video.duration||!Number.isFinite(video.duration)||video.duration>120)throw new Error('每段錄影最多 2 分鐘，請先裁短影片');
      const duration_ms=Math.ceil(video.duration*1000),pictures:Frame[]=[];
      const times:number[]=[];for(let at=0;at<duration_ms;at+=2000)times.push(at);const last=Math.max(0,duration_ms-100);if(last-times[times.length-1]>100)times.push(last);
      for(const at of times){if(!live.current)return;const seek=videoEvent(video,'seeked');video.currentTime=Math.max(.001,at/1000);await seek;pictures.push({at_ms:at,data_url:image(video)});setFrameCount(pictures.length);}
      if(live.current)replaceClip({blob:file,url:URL.createObjectURL(file),duration_ms,frames:pictures,id:uuid()});
    }catch(e){if(live.current){setError(e instanceof Error?e.message:'讀取影片失敗');setMode(clipRef.current?'ready':'idle');}}
    finally{URL.revokeObjectURL(url);video.removeAttribute('src');video.load();inFlight.current=false;}
  }
  async function attach(){
    if(!clip||inFlight.current)return;inFlight.current=true;setMode('uploading');setError('');
    try{
      setStage('加入錄影…');
      const name=title.trim()||'螢幕錄影';
      const saved=uploaded||await api.saveLesson(agent.id,{id:clip.id,title:name,notes,duration_ms:clip.duration_ms,frames:clip.frames,video:await base64(clip.blob)});setUploaded(saved);
      const label=name.replace(/[\[\]\\]/g,'\\$&');
      const text=[`[${label}](${saved.video_url})`,notes.trim(),saved.manifest_path?`示範資料：${saved.manifest_path}`:""].filter(Boolean).join('\n\n');
      if(live.current)onAttach(text);
    }catch(e){if(live.current){setError(e instanceof Error?e.message:'錄影加入失敗，可以重試');setMode('ready');}}
    finally{inFlight.current=false;}
  }
  const locked=mode==='uploading'||mode==='processing',canCapture=!!navigator.mediaDevices?.getDisplayMedia&&typeof MediaRecorder!=='undefined';
  function close(){if(mode==='recording'){stop();return;}if(!locked)onClose();}
  useDialogFocus(root,close);
  return <div className="modal-backdrop"><div ref={root} className="dialog recording-lesson" role="dialog" aria-modal="true" aria-label="錄影" onKeyDown={e=>{if(e.key==='Escape'){e.stopPropagation();close();}}}>
    <div className="skills-heading"><h2>錄影</h2><button type="button" disabled={locked} aria-label={mode==='recording'?'停止錄影並預覽':'關閉錄影'} onClick={close}>×</button></div>
    <label>錄影名稱（選填）<input value={title} maxLength={50} disabled={locked||!!uploaded} onChange={e=>setTitle(e.target.value)} placeholder="螢幕錄影"/></label>
    <p className="recording-hint">選擇螢幕或視窗開始錄影，完成後可下載或加入對話。每段最多 2 分鐘、32 MB。</p>
    {mode==='recording'?<div className="recording-live" role="status"><span>● 錄影中 · {Math.floor(elapsed/1000)} 秒</span><button type="button" onClick={()=>{try{capture();if(frames.current.length>=64)stop();}catch{setError('擷取失敗，請停止後檢查預覽');}}}>標記畫面</button><button type="button" onClick={stop}>停止錄影</button></div>:<div className="skill-actions"><button type="button" disabled={locked||!canCapture} onClick={()=>void start()}>{clip?'重新錄影':'開始螢幕錄影'}</button><button type="button" disabled={locked} onClick={()=>input.current?.click()}>上傳錄影</button>{clip?<a href={clip.url} download={`錄影.${clip.blob.type.includes('mp4')?'mp4':'webm'}`}>下載錄影</a>:null}</div>}
    {!canCapture?<small>此瀏覽器無法分享螢幕，可用 HTTPS／localhost 的桌面瀏覽器，或上傳已錄好的影片。</small>:null}
    <input hidden ref={input} type="file" accept="video/webm,video/mp4,.webm,.mp4" onChange={e=>{const file=e.target.files?.[0];e.target.value='';if(file)void importVideo(file);}}/>
    {clip&&mode!=='recording'?<><video className="lesson-preview" controls playsInline poster={clip.frames[0]?.data_url} src={clip.url} onLoadedMetadata={e=>{const video=e.currentTarget;if(!Number.isFinite(video.duration)){video.addEventListener('seeked',()=>{video.currentTime=0;},{once:true});video.currentTime=1e10;}}}/><small>{Math.ceil(clip.duration_ms/1000)} 秒</small></>:null}
    <label>補充說明（選填）<textarea rows={3} value={notes} maxLength={4000} disabled={locked||!!uploaded} onChange={e=>setNotes(e.target.value)} placeholder="有什麼希望他注意的地方？"/></label>
    {error?<p role="alert" className="pane-error">{error}</p>:null}
    {locked?<p role="status">{mode==='processing'?`處理錄影… ${frameCount}`:stage}</p>:null}
    <div className="dialog-actions"><button type="button" disabled={locked} onClick={close}>{mode==='recording'?'停止並預覽':'關閉'}</button><button type="button" className="primary" disabled={mode!=='ready'||!!clip&&clip.duration_ms>125000} onClick={()=>void attach()}>加入對話</button></div>
  </div></div>;
}
