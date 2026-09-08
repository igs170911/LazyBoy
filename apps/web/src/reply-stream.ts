import type { SessionEvent } from './live';

export interface ReplyDraft { runId:string; botId:string; generation:string; text:string; messageId?:string }
export type ReplyDrafts = Record<string,ReplyDraft>;

// Durable SSE ids remove duplicate frames in live.ts. A generation prevents a
// late frame from a failed attempt from joining the replacement attempt.
export function applyReplyEvent(current:ReplyDrafts,event:SessionEvent):ReplyDrafts {
  const {kind,payload}=event;
  if(kind==='session.cleared')return {};
  const runId=payload.runId;
  if(typeof runId!=='string')return current;
  if(kind==='reply.started'&&typeof payload.generation==='string'&&typeof payload.botId==='string'){
    return {...current,[runId]:{runId,botId:payload.botId,generation:payload.generation,text:''}};
  }
  const draft=current[runId];
  if(!draft)return current;
  if(kind==='message.created'&&payload.role==='assistant'&&typeof payload.id==='string'&&typeof payload.body==='string'){
    return {...current,[runId]:{...draft,text:payload.body,messageId:payload.id}};
  }
  if(draft.messageId)return current;
  if(kind==='reply.delta'&&payload.generation===draft.generation&&typeof payload.text==='string'){
    return {...current,[runId]:{...draft,text:draft.text+payload.text}};
  }
  if((kind==='reply.reset'&&payload.generation===draft.generation)||
    ['run.completed','run.failed','run.paused','run.cancelled'].includes(kind)){
    const next={...current};delete next[runId];return next;
  }
  return current;
}
