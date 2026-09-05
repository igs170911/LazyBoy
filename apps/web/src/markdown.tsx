import { memo, useCallback, useRef, useState, type ComponentPropsWithoutRef } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import { t } from "./i18n";
import "./markdown.css";

const protocolPattern = /^([a-z][a-z\d+.-]*):/i;
const safeProtocols = new Set(["http", "https", "mailto", "tel"]);

export function sanitizeMarkdownUrl(url: string): string | undefined {
  const value = url.trim();
  const protocol = value.match(protocolPattern)?.[1]?.toLowerCase();
  if (protocol) return safeProtocols.has(protocol) ? value : undefined;
  if (value.startsWith("#") && !value.toLowerCase().startsWith("#javascript")) return value;
  return undefined;
}

function CopyIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="9" y="9" width="12" height="12" rx="2" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"/>
      <path d="M5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/>
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path d="M4 12.5 9.5 18 20 6" stroke="currentColor" strokeWidth="2.25" strokeLinecap="round" strokeLinejoin="round"/>
    </svg>
  );
}

function useCopiedFlag() {
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<number>(undefined);
  const markCopied = useCallback(() => {
    setCopied(true);
    window.clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => setCopied(false), 1500);
  }, []);
  return {copied, markCopied};
}

function copyText(text: string): Promise<boolean> {
  if (!navigator.clipboard) return Promise.resolve(false);
  return navigator.clipboard.writeText(text).then(() => true).catch(() => false);
}

function CodeBlock(props: ComponentPropsWithoutRef<"pre">) {
  const preRef = useRef<HTMLPreElement>(null);
  const {copied, markCopied} = useCopiedFlag();
  const handleCopy = useCallback(() => {
    void copyText(preRef.current?.textContent ?? "").then((ok) => { if (ok) markCopied(); });
  }, [markCopied]);
  return (
    <div className="md-pre-wrap">
      <pre {...props} ref={preRef}/>
      <button type="button" className="md-copy" onClick={handleCopy} aria-label={copied ? t("copiedCode") : t("copyCode")}>
        {copied ? <CheckIcon/> : <CopyIcon/>}
      </button>
    </div>
  );
}

const components: Components = {
  a({node: _node, ...props}) {
    return <a {...props} target="_blank" rel="noreferrer noopener"/>;
  },
  img({node: _node, ...props}) {
    return <img {...props} alt={props.alt ?? ""} loading="lazy"/>;
  },
  pre({node: _node, ...props}) {
    return <CodeBlock {...props}/>;
  },
};

export function CopyMessageButton({text}:{text:string}) {
  const {copied, markCopied} = useCopiedFlag();
  if (!text.trim()) return null;
  return (
    <button
      type="button"
      className={`copy-msg ${copied ? "copied" : ""}`}
      title={copied ? t("copiedMessage") : t("copyMessage")}
      aria-label={copied ? t("copiedMessage") : t("copyMessage")}
      onClick={() => { void copyText(text).then((ok) => { if (ok) markCopied(); }); }}
    >
      {copied ? <CheckIcon/> : <CopyIcon/>}
      <span>{copied ? t("copiedMessage") : t("copyMessage")}</span>
    </button>
  );
}

export const ChatMarkdown = memo(function ChatMarkdown({children}:{children:string}) {
  return (
    <div className="md-body">
      <ReactMarkdown
        components={components}
        remarkPlugins={[remarkGfm, remarkBreaks]}
        skipHtml
        urlTransform={(url) => sanitizeMarkdownUrl(url) ?? ""}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
});
