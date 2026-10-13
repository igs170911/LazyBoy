import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "../ui/overlay";

type Alternative = { transcript: string };
type Result = { isFinal: boolean; 0: Alternative; length: number };
type ResultEvent = { resultIndex: number; results: { length: number; [index: number]: Result } };
type Recognition = {
  lang: string; continuous: boolean; interimResults: boolean;
  start(): void; stop(): void; abort(): void;
  onresult: ((event: ResultEvent) => void) | null;
  onerror: ((event: { error: string }) => void) | null;
  onend: (() => void) | null;
};
type RecognitionClass = new () => Recognition;

const Recognizer: RecognitionClass | undefined = typeof window === "undefined" ? undefined
  : (window as unknown as { SpeechRecognition?: RecognitionClass; webkitSpeechRecognition?: RecognitionClass }).SpeechRecognition
    || (window as unknown as { webkitSpeechRecognition?: RecognitionClass }).webkitSpeechRecognition;

export const dictationSupported = !!Recognizer;

const ERRORS: Record<string, string> = {
  "not-allowed": "請允許瀏覽器使用麥克風",
  "service-not-allowed": "這個瀏覽器沒有開放語音輸入",
  "audio-capture": "找不到麥克風",
  network: "語音辨識需要網路連線",
};

/**
 * Grok's "Start voice input" (Ctrl/⌘+D): speech becomes text in the composer, to edit before sending.
 * Uses the browser's own speech recognition; finished phrases are handed to `onText` as they are recognized.
 */
export function useDictation(onText: (text: string) => void) {
  const [listening, setListening] = useState(false);
  const [interim, setInterim] = useState("");
  const recognition = useRef<Recognition | null>(null);
  const deliver = useRef(onText);
  deliver.current = onText;

  const stop = useCallback(() => { recognition.current?.stop(); }, []);
  const start = useCallback(() => {
    if (recognition.current) return;
    if (!Recognizer) { toast("這個瀏覽器不支援語音輸入，可以改用 Chrome、Edge 或 Safari", "error"); return; }
    const r = new Recognizer();
    const language = navigator.language || "zh-TW";
    r.lang = language.toLowerCase().startsWith("zh") ? "zh-TW" : language;
    r.continuous = true;
    r.interimResults = true;
    r.onresult = event => {
      let pending = "";
      for (let i = event.resultIndex; i < event.results.length; i++) {
        const result = event.results[i];
        if (result.isFinal) deliver.current(result[0].transcript);
        else pending += result[0].transcript;
      }
      setInterim(pending);
    };
    r.onerror = event => {
      if (event.error !== "aborted" && event.error !== "no-speech") toast(ERRORS[event.error] || `語音輸入失敗（${event.error}）`, "error");
    };
    r.onend = () => { recognition.current = null; setListening(false); setInterim(""); };
    recognition.current = r;
    try { r.start(); setListening(true); }
    catch { recognition.current = null; toast("無法開始語音輸入", "error"); }
  }, []);
  const toggle = useCallback(() => { if (recognition.current) stop(); else start(); }, [start, stop]);
  useEffect(() => () => { recognition.current?.abort(); }, []);
  return { listening, interim, toggle, stop };
}
