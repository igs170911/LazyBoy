import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "../ui/overlay";

type Alternative = { transcript: string };
type ResultList = { length: number; [index: number]: { isFinal: boolean; 0: Alternative } };
type Recognition = {
  lang: string; continuous: boolean; interimResults: boolean;
  start(): void; stop(): void; abort(): void;
  onresult: ((event: { resultIndex: number; results: ResultList }) => void) | null;
  onerror: ((event: { error: string }) => void) | null;
  onend: (() => void) | null;
};
type RecognitionClass = new () => Recognition;

const Recognizer: RecognitionClass | undefined = typeof window === "undefined" ? undefined
  : (window as unknown as { SpeechRecognition?: RecognitionClass }).SpeechRecognition
    || (window as unknown as { webkitSpeechRecognition?: RecognitionClass }).webkitSpeechRecognition;

export const speechSupported = typeof window !== "undefined" && "speechSynthesis" in window;
export const voiceChatSupported = !!Recognizer && speechSupported;

function language() {
  const lang = typeof navigator !== "undefined" ? navigator.language || "zh-TW" : "zh-TW";
  return lang.toLowerCase().startsWith("zh") ? "zh-TW" : lang;
}

/** Markdown turned into something worth hearing: no code blocks, links read by their words, a sane length. */
export function speakable(markdown: string, limit = 1200) {
  const text = markdown
    .replace(/```[\s\S]*?```/g, "（程式碼略過）")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^>\s?/gm, "")
    .replace(/[#*_`~|]/g, "")
    .replace(/\n{2,}/g, "。\n")
    .trim();
  return text.length > limit ? `${text.slice(0, limit)}……以下省略，請看文字。` : text;
}

/** Read text aloud with a Chinese voice when the system has one; `onEnd` runs when it finishes or is stopped. */
export function speak(text: string, onEnd?: () => void) {
  if (!speechSupported || !text.trim()) { onEnd?.(); return; }
  window.speechSynthesis.cancel();
  const utterance = new SpeechSynthesisUtterance(text);
  const lang = language();
  utterance.lang = lang;
  const voice = window.speechSynthesis.getVoices().find(v => v.lang.replace("_", "-").toLowerCase() === lang.toLowerCase())
    || window.speechSynthesis.getVoices().find(v => v.lang.toLowerCase().startsWith(lang.slice(0, 2).toLowerCase()));
  if (voice) utterance.voice = voice;
  utterance.onend = () => onEnd?.();
  utterance.onerror = () => onEnd?.();
  window.speechSynthesis.speak(utterance);
}

export function stopSpeaking() {
  if (speechSupported) window.speechSynthesis.cancel();
}

/** One spoken message, ended by a pause; `onText` gets the words (empty when nothing was said). */
function listenOnce(onText: (text: string) => void, onInterim: (text: string) => void): Recognition | null {
  if (!Recognizer) return null;
  const r = new Recognizer();
  r.lang = language();
  r.continuous = false;
  r.interimResults = true;
  let final = "";
  r.onresult = event => {
    let pending = "";
    for (let i = event.resultIndex; i < event.results.length; i++) {
      const result = event.results[i];
      if (result.isFinal) final += result[0].transcript; else pending += result[0].transcript;
    }
    onInterim(final + pending);
  };
  r.onerror = event => {
    if (event.error === "not-allowed" || event.error === "service-not-allowed") toast("請允許瀏覽器使用麥克風", "error");
  };
  r.onend = () => onText(final.trim());
  r.start();
  return r;
}

export type VoicePhase = "off" | "listening" | "waiting" | "speaking";

/**
 * Grok's "Start voice chat", built from the browser's own speech tools: listen until a pause, send what was said,
 * read the bot's reply aloud, then listen again, until stopped.
 */
export function useVoiceChat({ send, reply }: { send: (text: string) => Promise<void>; reply: { id?: string; content: string } | null }) {
  const [phase, setPhase] = useState<VoicePhase>("off");
  const [heard, setHeard] = useState("");
  const recognition = useRef<Recognition | null>(null);
  const awaiting = useRef<string | null>(null);
  const phaseRef = useRef<VoicePhase>("off");
  phaseRef.current = phase;

  const listen = useCallback(() => {
    setHeard(""); setPhase("listening");
    recognition.current = listenOnce(text => {
      recognition.current = null;
      if (phaseRef.current !== "listening") return;
      if (!text) { listen(); return; }  // silence: keep listening
      setPhase("waiting");
      awaiting.current = reply?.id || "";
      void send(text).catch(err => { toast(err instanceof Error ? err.message : String(err), "error"); setPhase("off"); });
    }, setHeard);
    if (!recognition.current) setPhase("off");
  }, [send, reply?.id]);

  const stop = useCallback(() => {
    setPhase("off"); setHeard("");
    recognition.current?.abort(); recognition.current = null;
    awaiting.current = null;
    stopSpeaking();
  }, []);

  const start = useCallback(() => {
    if (!voiceChatSupported) { toast("這個瀏覽器不支援語音對話，可以改用 Chrome、Edge 或 Safari", "error"); return; }
    listen();
  }, [listen]);

  // A new finished reply after what we sent: read it, then listen again.
  useEffect(() => {
    if (phase !== "waiting" || !reply?.id || reply.id === awaiting.current) return;
    awaiting.current = reply.id;
    setPhase("speaking");
    speak(speakable(reply.content), () => { if (phaseRef.current === "speaking") listen(); });
  }, [phase, reply, listen]);

  useEffect(() => () => { recognition.current?.abort(); stopSpeaking(); }, []);
  return { phase, heard, start, stop };
}
