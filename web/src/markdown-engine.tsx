import { isValidElement, useEffect, useMemo, useRef, useState, type ReactElement, type ReactNode } from "react";
import ReactMarkdown, { defaultUrlTransform, type Components } from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkCjkFriendly from "remark-cjk-friendly";
import remarkCjkFriendlyGfmStrikethrough from "remark-cjk-friendly-gfm-strikethrough";
import remarkGfm from "remark-gfm";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import css from "highlight.js/lib/languages/css";
import diff from "highlight.js/lib/languages/diff";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import markdown from "highlight.js/lib/languages/markdown";
import python from "highlight.js/lib/languages/python";
import shell from "highlight.js/lib/languages/shell";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

for (const [name, language] of Object.entries({ bash, css, diff, javascript, json, markdown, python, shell, sql, typescript, xml, yaml })) {
  hljs.registerLanguage(name, language);
}

type MdNode = { type: string; value?: string; children?: MdNode[]; data?: { hProperties?: Record<string, unknown> } };
const ALERTS = ["note", "tip", "important", "warning", "caution"];

/** GitHub-style `> [!NOTE]` blockquotes become callouts; the marker line is removed. */
function remarkAlerts() {
  const walk = (node: MdNode) => {
    if (node.type === "blockquote") {
      const first = node.children?.[0];
      const lead = first?.type === "paragraph" ? first.children?.[0] : undefined;
      const match = lead?.type === "text" ? /^\[!(\w+)\][ \t]*/.exec(lead.value || "") : null;
      const kind = match?.[1].toLowerCase();
      if (first && lead && match && kind && ALERTS.includes(kind)) {
        lead.value = (lead.value || "").slice(match[0].length).replace(/^\n/, "");
        if (!lead.value) {
          first.children!.shift();
          if (first.children![0]?.type === "break") first.children!.shift();
        }
        if (!first.children!.length) node.children!.shift();
        node.data = { hProperties: { className: ["md-alert"], "data-alert": kind } };
      }
    }
    node.children?.forEach(walk);
  };
  return (tree: MdNode) => walk(tree);
}

/** Full-width punctuation never belongs to a web address. */
const FULLWIDTH_STOP = /[‘’“”…　-〿！-／：-＠［-｀｛-･]/u;

/**
 * GFM's bare-URL autolink runs on until whitespace, and Chinese puts none after a URL, so `網址），後面的話` became
 * one link and the sentence after it vanished behind the link's short label. End the link at full-width punctuation.
 */
function remarkCjkUrlEnd() {
  const walk = (node: MdNode) => {
    const children = node.children;
    if (!children) return;
    for (let index = 0; index < children.length; index++) {
      const link = children[index] as MdNode & { url?: string };
      const label = link.type === "link" && link.children?.length === 1 && link.children[0].type === "text" ? link.children[0].value || "" : "";
      const stop = label.search(FULLWIDTH_STOP);
      if (stop <= 0 || !link.url?.endsWith(label)) {
        walk(link);
        continue;
      }
      // Like GFM, trailing ASCII punctuation and an unmatched ")" stay outside the link.
      let head = label.slice(0, stop).replace(/[.,:;!?*_~'"]+$/, "");
      while (head.endsWith(")") && head.split(")").length > head.split("(").length) head = head.slice(0, -1);
      if (head.length < 4) continue;
      link.url = link.url.slice(0, link.url.length - label.length) + head;
      link.children = [{ type: "text", value: head }];
      children.splice(index + 1, 0, { type: "text", value: label.slice(head.length) });
      index++;
    }
  };
  return (tree: MdNode) => walk(tree);
}

// Hermes replies are chat text: single newlines are intentional, and CJK punctuation next to ** must still emphasize.
const PLUGINS = [remarkGfm, remarkCjkFriendly, remarkCjkFriendlyGfmStrikethrough, remarkBreaks, remarkAlerts, remarkCjkUrlEnd];

function CodeBlock({ children }: { children?: ReactNode }) {
  const child = (Array.isArray(children) ? children[0] : children) as ReactElement<{ className?: string; children?: ReactNode }> | undefined;
  const raw = isValidElement(child) ? child.props.children : children;
  const text = (Array.isArray(raw) ? raw.join("") : String(raw ?? "")).replace(/\n$/, "");
  const declared = isValidElement(child) ? /language-([\w+#-]+)/.exec(child.props.className || "")?.[1]?.toLowerCase() : undefined;
  const html = useMemo(() => {
    if (!declared || !hljs.getLanguage(declared) || text.length > 60000) return null;
    try { return hljs.highlight(text, { language: declared, ignoreIllegals: true }).value; } catch { return null; }
  }, [declared, text]);
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const area = document.createElement("textarea");
      area.value = text; document.body.append(area); area.select();
      document.execCommand("copy"); area.remove();
    }
    setCopied(true);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(false), 1600);
  };
  return (
    <div className="md-code">
      <div className="md-code-head">
        <span className="md-code-lang">{declared || "文字"}</span>
        <button type="button" className="md-code-copy" onClick={() => void copy()} aria-label={copied ? "已複製" : "複製程式碼"}>
          {copied ? "已複製" : "複製"}
        </button>
      </div>
      {html !== null
        ? <pre><code className={`hljs language-${declared}`} dangerouslySetInnerHTML={{ __html: html }} /></pre>
        : <pre><code>{text}</code></pre>}
    </div>
  );
}

export default function MarkdownEngine({ children, components }: { children: string; components: Components }) {
  const merged = useMemo<Components>(() => ({ pre: CodeBlock, ...components }), [components]);
  return <ReactMarkdown remarkPlugins={PLUGINS} components={merged} urlTransform={url =>
    url.includes("#file=") ? url : defaultUrlTransform(url)
  }>{children}</ReactMarkdown>;
}
