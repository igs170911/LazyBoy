import { lazy, Suspense, useEffect, useMemo, useState, type ReactNode } from "react";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import diff from "highlight.js/lib/languages/diff";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import lua from "highlight.js/lib/languages/lua";
import makefile from "highlight.js/lib/languages/makefile";
import markdown from "highlight.js/lib/languages/markdown";
import php from "highlight.js/lib/languages/php";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";
import r from "highlight.js/lib/languages/r";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import scss from "highlight.js/lib/languages/scss";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";
import { contentUrl, MarkdownBody, toContentUrl, type PreviewFile } from "./markdown";
import { bytes } from "./workspace/format";

for (const [name, language] of Object.entries({ bash, c, cpp, csharp, css, diff, dockerfile, go, ini, java, javascript, json, kotlin, lua, makefile, markdown, php, powershell, python, r, ruby, rust, scss, sql, swift, typescript, xml, yaml })) {
  hljs.registerLanguage(name, language);
}

const PdfPages = lazy(() => import("./pdf-pages"));

/** How much of a text file is shown; the rest is one download away. */
const TEXT_LIMIT = 2 * 1024 * 1024;
const TABLE_ROWS = 1000;
const HIGHLIGHT_LIMIT = 400 * 1024;

const CODE: Record<string, string> = {
  js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "javascript", ts: "typescript", tsx: "typescript", mts: "typescript", cts: "typescript",
  py: "python", pyw: "python", pyi: "python", sh: "bash", bash: "bash", zsh: "bash", fish: "bash", css: "css", scss: "scss", sass: "scss", less: "scss",
  xml: "xml", xsl: "xml", xsd: "xml", plist: "xml", vue: "xml", svelte: "xml", yaml: "yaml", yml: "yaml", toml: "ini", ini: "ini", cfg: "ini",
  conf: "ini", env: "ini", properties: "ini", sql: "sql", go: "go", rs: "rust", java: "java", kt: "kotlin", kts: "kotlin", swift: "swift",
  c: "c", h: "c", cpp: "cpp", cc: "cpp", cxx: "cpp", hpp: "cpp", hh: "cpp", cs: "csharp", php: "php", rb: "ruby", lua: "lua", r: "r",
  ps1: "powershell", psm1: "powershell", diff: "diff", patch: "diff", dockerfile: "dockerfile", makefile: "makefile", mk: "makefile",
  jsonl: "json", ndjson: "json", jsonc: "json", json5: "json", gradle: "java", scala: "java", dart: "java", graphql: "", gql: "", proto: "",
  tex: "", bib: "", srt: "", vtt: "", ass: "", nfo: "", txt: "", text: "", log: "", out: "", rst: "", adoc: "", org: "", gitignore: "ini",
};
const NAMES: Record<string, string> = { dockerfile: "dockerfile", makefile: "makefile", gnumakefile: "makefile", procfile: "bash", readme: "", license: "", changelog: "", authors: "" };
const IMAGE = new Set(["png", "jpg", "jpeg", "jfif", "gif", "webp", "avif", "bmp", "ico", "apng", "tif", "tiff", "heic", "heif"]);
const VIDEO = new Set(["mp4", "m4v", "webm", "mov", "ogv", "mkv", "3gp"]);
const AUDIO = new Set(["mp3", "wav", "m4a", "aac", "ogg", "oga", "opus", "flac", "weba", "amr"]);
const FONT = new Set(["ttf", "otf", "woff", "woff2"]);
const ARCHIVE = new Set(["zip", "jar", "war", "apk", "whl", "epub", "tar", "tgz", "tbz", "tbz2", "txz", "gz", "bz2", "xz"]);
const OFFICE = new Set(["doc", "docx", "docm", "dot", "dotx", "odt", "ott", "rtf", "wpd", "pages", "ppt", "pptx", "pptm", "pps", "ppsx", "odp", "otp",
  "key", "vsd", "vsdx", "odg", "pub", "xls", "xlsx", "xlsm", "xlsb", "ods", "ots", "numbers"]);

type Kind = "markdown" | "code" | "csv" | "json" | "notebook" | "html" | "image" | "svg" | "video" | "audio" | "pdf" | "office" | "archive" | "font" | "sniff";

function classify(file: PreviewFile): { kind: Kind; language?: string } {
  const name = (file.path || file.title).split("/").pop()!.toLowerCase();
  const ext = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1) : "";
  const type = (file.mediaType || "").toLowerCase();
  if (ext === "md" || ext === "markdown" || ext === "mdx" || type === "text/markdown") return { kind: "markdown" };
  if (ext === "csv" || ext === "tsv" || type === "text/csv") return { kind: "csv" };
  if (ext === "json" || ext === "geojson" || ext === "webmanifest" || type === "application/json") return { kind: "json" };
  if (ext === "ipynb") return { kind: "notebook" };
  if (ext === "html" || ext === "htm" || ext === "xhtml" || type === "text/html") return { kind: "html" };
  if (ext === "svg" || type === "image/svg+xml") return { kind: "svg" };
  if (IMAGE.has(ext) || type.startsWith("image/")) return { kind: "image" };
  if (VIDEO.has(ext) || type.startsWith("video/")) return { kind: "video" };
  if (AUDIO.has(ext) || type.startsWith("audio/")) return { kind: "audio" };
  if (ext === "pdf" || type === "application/pdf") return { kind: "pdf" };
  if (OFFICE.has(ext)) return { kind: "office" };
  if (FONT.has(ext) || type.startsWith("font/")) return { kind: "font" };
  if (ARCHIVE.has(ext) || name.endsWith(".tar.gz")) return { kind: "archive" };
  if (ext in CODE) return { kind: "code", language: CODE[ext] };
  const bare = name.replace(/\..*$/, "");
  if (!ext && bare in NAMES) return { kind: "code", language: NAMES[bare] };
  if (name.startsWith(".") && !name.slice(1).includes(".")) return { kind: "code", language: "ini" };
  if (type.startsWith("text/") || type.endsWith("+json") || type.endsWith("+xml")) return { kind: "code", language: "" };
  return { kind: "sniff" };
}

/** Kinds with both a rendered view and the raw text behind it. */
const HAS_SOURCE: Kind[] = ["markdown", "csv", "json", "notebook", "html", "svg"];

type Loaded = { text: string; truncated: boolean };

/**
 * Up to `limit` bytes of a file as text, or null when it is not text. UTF-8 first, then the encodings files made on
 * Taiwanese and Chinese Windows machines use (Big5, GB18030), and UTF-16 when it carries a byte-order mark.
 */
async function readText(url: string, limit = TEXT_LIMIT, signal?: AbortSignal): Promise<Loaded | null> {
  const response = await fetch(url, { signal });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const reader = response.body?.getReader();
  const parts: Uint8Array[] = [];
  let size = 0, truncated = false;
  if (reader) {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      parts.push(value); size += value.length;
      if (size > limit) { truncated = true; void reader.cancel(); break; }
    }
  } else {
    const all = new Uint8Array(await response.arrayBuffer());
    parts.push(all); size = all.length;
  }
  const data = new Uint8Array(Math.min(size, limit));
  let offset = 0;
  for (const part of parts) {
    const take = Math.min(part.length, data.length - offset);
    data.set(part.subarray(0, take), offset); offset += take;
    if (offset >= data.length) break;
  }
  const text = decode(data, truncated);
  return text === null ? null : { text, truncated };
}

function decode(data: Uint8Array, truncated: boolean): string | null {
  if (data[0] === 0xff && data[1] === 0xfe) return new TextDecoder("utf-16le").decode(data);
  if (data[0] === 0xfe && data[1] === 0xff) return new TextDecoder("utf-16be").decode(data);
  const sample = data.subarray(0, 8192);
  if (sample.includes(0)) return null;
  let control = 0;
  for (const byte of sample) if (byte < 9 || (byte > 13 && byte < 32)) control++;
  if (control > sample.length * 0.02) return null;
  // A cut-off file may end in the middle of a character.
  const body = truncated ? data.subarray(0, Math.max(0, data.length - 4)) : data;
  for (const encoding of ["utf-8", "big5", "gb18030"]) {
    try { return new TextDecoder(encoding, { fatal: true }).decode(body); } catch { /* next */ }
  }
  return new TextDecoder("utf-8").decode(body);
}

function useText(url: string, enabled: boolean) {
  const [state, setState] = useState<{ status: "loading" | "ready" | "binary" | "error"; loaded?: Loaded }>({ status: "loading" });
  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    setState({ status: "loading" });
    readText(url, TEXT_LIMIT, controller.signal)
      .then(loaded => setState(loaded ? { status: "ready", loaded } : { status: "binary" }))
      .catch(error => { if (!controller.signal.aborted) setState({ status: "error" }); void error; });
    return () => controller.abort();
  }, [url, enabled]);
  return state;
}

function Status({ children }: { children: ReactNode }) {
  return <p className="file-viewer-status">{children}</p>;
}

function Cut({ loaded }: { loaded?: Loaded }) {
  return loaded?.truncated ? <p className="file-cut">檔案較大，只顯示前 {bytes(TEXT_LIMIT)}。完整內容請下載。</p> : null;
}

function Code({ text, language }: { text: string; language?: string }) {
  const html = useMemo(() => {
    if (text.length > HIGHLIGHT_LIMIT) return null;
    try {
      if (language && hljs.getLanguage(language)) return hljs.highlight(text, { language, ignoreIllegals: true }).value;
    } catch { /* plain */ }
    return null;
  }, [text, language]);
  return html !== null
    ? <pre className="file-text file-code"><code className="hljs" dangerouslySetInnerHTML={{ __html: html }} /></pre>
    : <pre className="file-text">{text}</pre>;
}

/** RFC 4180 rows: quoted fields may hold the separator, quotes ("") and line breaks. */
function parseTable(text: string, separator: string, limit: number) {
  const rows: string[][] = [];
  let row: string[] = [], field = "", quoted = false, i = 0;
  const source = text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
  for (; i < source.length && rows.length < limit; i++) {
    const ch = source[i];
    if (quoted) {
      if (ch === '"') { if (source[i + 1] === '"') { field += '"'; i++; } else quoted = false; }
      else field += ch;
    } else if (ch === '"' && !field) quoted = true;
    else if (ch === separator) { row.push(field); field = ""; }
    else if (ch === "\n" || ch === "\r") {
      if (ch === "\r" && source[i + 1] === "\n") i++;
      row.push(field); rows.push(row); row = []; field = "";
    } else field += ch;
  }
  if ((field || row.length) && rows.length < limit) { row.push(field); rows.push(row); }
  return { rows, more: i < source.length };
}

function Table({ text, separator }: { text: string; separator: string }) {
  const { rows, more } = useMemo(() => parseTable(text, separator, TABLE_ROWS + 1), [text, separator]);
  if (!rows.length) return <Status>這個表格是空的。</Status>;
  const [head, ...body] = rows.slice(0, TABLE_ROWS + 1);
  const width = Math.max(...rows.map(row => row.length));
  return (
    <>
      <div className="file-table-wrap">
        <table className="file-table">
          <thead><tr><th aria-label="列" />{Array.from({ length: width }, (_, i) => <th key={i}>{head[i] ?? ""}</th>)}</tr></thead>
          <tbody>{body.map((row, r) => <tr key={r}><td className="file-table-n">{r + 1}</td>{Array.from({ length: width }, (_, i) => <td key={i}>{row[i] ?? ""}</td>)}</tr>)}</tbody>
        </table>
      </div>
      {more ? <p className="file-cut">只顯示前 {TABLE_ROWS} 列。完整內容請下載。</p> : null}
    </>
  );
}

type Cell = { cell_type: string; source: string | string[]; outputs?: { output_type: string; text?: string | string[]; data?: Record<string, string | string[]>; ename?: string; evalue?: string }[] };
const joined = (value?: string | string[]) => Array.isArray(value) ? value.join("") : value || "";

function Notebook({ text, agentId, resolveUrl }: { text: string; agentId?: string; resolveUrl: (src: string) => string | undefined }) {
  const parsed = useMemo(() => { try { return JSON.parse(text) as { cells?: Cell[]; metadata?: { language_info?: { name?: string } } }; } catch { return null; } }, [text]);
  if (!parsed?.cells) return <Code text={text} language="json" />;
  const language = parsed.metadata?.language_info?.name || "python";
  return (
    <div className="file-notebook">
      {parsed.cells.map((cell, index) => cell.cell_type === "markdown"
        ? <div key={index} className="nb-markdown"><MarkdownBody content={joined(cell.source)} agentId={agentId} resolveUrl={resolveUrl} /></div>
        : (
          <div key={index} className="nb-cell">
            <Code text={joined(cell.source)} language={cell.cell_type === "code" ? language : ""} />
            {(cell.outputs || []).map((output, n) => {
              const image = output.data?.["image/png"] || output.data?.["image/jpeg"];
              if (image) return <img key={n} className="nb-image" alt="" src={`data:${output.data?.["image/png"] ? "image/png" : "image/jpeg"};base64,${joined(image).replace(/\s/g, "")}`} />;
              const value = output.output_type === "error" ? `${output.ename}: ${output.evalue}` : joined(output.text) || joined(output.data?.["text/plain"]);
              return value ? <pre key={n} className={`nb-output${output.output_type === "error" ? " is-error" : ""}`}>{value}</pre> : null;
            })}
          </div>
        ))}
    </div>
  );
}

const external = (value: string) => /^(?:[a-z][a-z0-9+.-]*:|\/\/|#)/i.test(value.trim());
const dataUrl = (blob: Blob) => new Promise<string>((resolve, reject) => {
  const reader = new FileReader();
  reader.onload = () => resolve(String(reader.result)); reader.onerror = () => reject(reader.error); reader.readAsDataURL(blob);
});

/**
 * A page the bot made, shown in a sandbox with no access to LazyBoy. The sandboxed page cannot use this site's login,
 * so its own stylesheets, scripts and images from the bot's files are fetched here and written into the page.
 */
async function selfContained(html: string, resolve: (src: string) => string | undefined, css?: string): Promise<string> {
  const doc = new DOMParser().parseFromString(html, "text/html");
  let budget = 30 * 1024 * 1024;
  const get = async (src: string) => {
    const url = resolve(src);
    if (!url || budget <= 0) return null;
    const response = await fetch(url);
    if (!response.ok) return null;
    const blob = await response.blob();
    budget -= blob.size;
    return blob;
  };
  const jobs: Promise<void>[] = [];
  doc.querySelectorAll<HTMLLinkElement>('link[rel~="stylesheet"][href]').forEach(link => {
    const href = link.getAttribute("href")!;
    if (external(href)) return;
    jobs.push(get(href).then(async blob => { if (!blob) return; const style = doc.createElement("style"); style.textContent = await blob.text(); link.replaceWith(style); }).catch(() => {}));
  });
  doc.querySelectorAll<HTMLScriptElement>("script[src]").forEach(script => {
    const src = script.getAttribute("src")!;
    if (external(src)) return;
    jobs.push(get(src).then(async blob => { if (!blob) return; script.removeAttribute("src"); script.textContent = await blob.text(); }).catch(() => {}));
  });
  doc.querySelectorAll<HTMLElement>("img[src], source[src], video[poster], audio[src], video[src]").forEach(node => {
    for (const attribute of ["src", "poster"]) {
      const value = node.getAttribute(attribute);
      if (!value || external(value)) continue;
      jobs.push(get(value).then(async blob => { if (blob) node.setAttribute(attribute, await dataUrl(blob)); }).catch(() => {}));
    }
  });
  await Promise.all(jobs);
  // Links leave the sandbox in a new tab instead of replacing the preview.
  const base = doc.createElement("base"); base.target = "_blank"; doc.head.prepend(base);
  if (css) { const style = doc.createElement("style"); style.textContent = css; doc.head.append(style); }
  return "<!doctype html>\n" + doc.documentElement.outerHTML;
}

/** A converted spreadsheet carries everything inline. One constant, so a re-render does not rebuild the page. */
const NO_RESOURCES = () => undefined;

/** Spreadsheets exported by LibreOffice: keep columns readable and let the sheet scroll sideways on a phone. */
const SHEET_CSS = `body{margin:8px;font-family:system-ui,-apple-system,"PingFang TC","Noto Sans TC",sans-serif;font-size:13px;color:#1f2328;background:#fff}
table{border-collapse:collapse;width:auto!important;margin-bottom:16px}td,th{min-width:5.5em;max-width:22em;padding:4px 6px;border:1px solid #d8dbe0;vertical-align:top;font-family:inherit!important}
body *{font-family:inherit!important}h1,h2,h3{font-size:14px;margin:12px 0 6px}`;

function HtmlPage({ html, resolve, scripts = true, title, css }: { html: string; resolve: (src: string) => string | undefined; scripts?: boolean; title: string; css?: string }) {
  const [page, setPage] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    setPage(null);
    selfContained(html, resolve, css).then(value => { if (alive) setPage(value); }).catch(() => { if (alive) setPage(html); });
    return () => { alive = false; };
  }, [html, resolve, css]);
  if (page === null) return <Status>正在準備頁面…</Status>;
  return <iframe className="file-frame" title={title} srcDoc={page} sandbox={scripts ? "allow-scripts allow-popups allow-popups-to-escape-sandbox" : "allow-popups allow-popups-to-escape-sandbox"} />;
}

function isTouch() {
  return window.matchMedia("(pointer: coarse)").matches || /iP(hone|ad|od)/.test(navigator.userAgent);
}

/** PDFs: the browser's own viewer on a computer; drawn page by page on phones, whose viewers show only page one. */
function Pdf({ url, title }: { url: string; title: string }) {
  if (!isTouch()) return <iframe className="file-frame" title={title} src={url} />;
  return <Suspense fallback={<Status>正在載入 PDF…</Status>}><PdfPages url={url} /></Suspense>;
}

function Office({ agentId, file }: { agentId?: string; file: PreviewFile }) {
  const [state, setState] = useState<{ status: "loading" } | { status: "error"; error: string } | { status: "pdf"; url: string } | { status: "html"; html: string }>({ status: "loading" });
  useEffect(() => {
    if (!agentId || !file.path) { setState({ status: "error", error: "無法預覽這個檔案" }); return; }
    const controller = new AbortController();
    let objectUrl = "";
    setState({ status: "loading" });
    fetch(`/api/agents/${encodeURIComponent(agentId)}/files/office?path=${encodeURIComponent(file.path)}`, { signal: controller.signal })
      .then(async response => {
        if (!response.ok) throw new Error(((await response.json().catch(() => null)) as { error?: string } | null)?.error || `HTTP ${response.status}`);
        if (response.headers.get("X-Preview-Kind") === "html") setState({ status: "html", html: await response.text() });
        else { objectUrl = URL.createObjectURL(await response.blob()); setState({ status: "pdf", url: objectUrl }); }
      })
      .catch(error => { if (!controller.signal.aborted) setState({ status: "error", error: error instanceof Error ? error.message : String(error) }); });
    return () => { controller.abort(); if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [agentId, file.path]);
  if (state.status === "loading") return <Status>正在用 Bot 的電腦開啟這份文件，第一次可能要十幾秒…</Status>;
  if (state.status === "error") return <Fallback file={file} reason={state.error} />;
  if (state.status === "html") return <HtmlPage html={state.html} resolve={NO_RESOURCES} scripts={false} title={file.title} css={SHEET_CSS} />;
  return <Pdf url={state.url} title={file.title} />;
}

function Archive({ agentId, file }: { agentId?: string; file: PreviewFile }) {
  const [data, setData] = useState<{ entries: { name: string; bytes: number; dir: boolean }[]; total: number; truncated: boolean } | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!agentId || !file.path) return;
    setData(null); setError("");
    // An answer for a file already left behind must not replace the one on screen.
    const controller = new AbortController();
    fetch(`/api/agents/${encodeURIComponent(agentId)}/files/archive?path=${encodeURIComponent(file.path)}`, { signal: controller.signal })
      .then(async response => { const body = await response.json(); if (!response.ok) throw new Error(body.error || `HTTP ${response.status}`); setData(body); })
      .catch(err => { if (!controller.signal.aborted) setError(err instanceof Error ? err.message : String(err)); });
    return () => controller.abort();
  }, [agentId, file.path]);
  if (error) return <Fallback file={file} reason={error} />;
  if (!data) return <Status>讀取壓縮檔內容…</Status>;
  const files = data.entries.filter(entry => !entry.dir);
  return (
    <div className="file-archive">
      <p className="file-cut">共 {data.total} 個項目，{files.length} 個檔案，解壓後約 {bytes(files.reduce((sum, entry) => sum + entry.bytes, 0))}{data.truncated ? `（只列出前 ${data.entries.length} 個）` : ""}。</p>
      <ul>{data.entries.map(entry => <li key={entry.name} data-dir={entry.dir || undefined}><span>{entry.name}</span>{entry.dir ? null : <small>{bytes(entry.bytes)}</small>}</li>)}</ul>
    </div>
  );
}

function FontSample({ url, title }: { url: string; title: string }) {
  const [family, setFamily] = useState("");
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    const name = `preview-${Math.random().toString(36).slice(2)}`;
    const face = new FontFace(name, `url(${JSON.stringify(url)})`);
    face.load().then(loaded => { document.fonts.add(loaded); setFamily(name); }).catch(() => setFailed(true));
    return () => { document.fonts.delete(face); };
  }, [url]);
  if (failed) return <Status>這個字型無法載入。</Status>;
  if (!family) return <Status>載入字型…</Status>;
  return (
    <div className="file-font" style={{ fontFamily: family }} aria-label={`${title} 字型範例`}>
      {[44, 28, 18, 14].map(size => <p key={size} style={{ fontSize: size }}>LazyBoy 你好，今天天氣很好。The quick brown fox 0123456789</p>)}
    </div>
  );
}

function Media({ kind, url, file }: { kind: "image" | "video" | "audio" | "svg"; url: string; file: PreviewFile }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <Fallback file={file} reason={kind === "image" ? "這個瀏覽器無法顯示這種圖片格式" : "這個瀏覽器無法播放這種格式"} />;
  if (kind === "video") return <video className="file-media" src={url} controls playsInline preload="metadata" onError={() => setFailed(true)} />;
  if (kind === "audio") return <div className="file-audio"><strong>{file.title}</strong><audio src={url} controls preload="metadata" onError={() => setFailed(true)} /></div>;
  return <a className="file-image" href={url} target="_blank" rel="noreferrer"><img src={url} alt={file.title} onError={() => setFailed(true)} /></a>;
}

function Fallback({ file, reason }: { file: PreviewFile; reason?: string }) {
  const name = file.title;
  const ext = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1).toUpperCase() : "";
  return (
    <div className="file-fallback">
      <span className="file-fallback-badge" aria-hidden="true">{ext.slice(0, 5) || "FILE"}</span>
      <strong>{reason || "這個檔案無法在網頁上預覽"}</strong>
      <small>{[file.mediaType, ext ? `.${ext.toLowerCase()} 檔` : ""].filter(Boolean)[0] || "未知格式"}。可以下載後用電腦上的程式開啟，或請 Bot 幫你轉成其他格式。</small>
    </div>
  );
}

/** The best view this browser can give a file, falling back to plain text, then to its details and a download. */
type FolderEntry = { name: string; path: string; dir: boolean; bytes: number | null };
type Stat = { kind: "file" } | { kind: "folder"; entries: FolderEntry[]; truncated: boolean } | { kind: "missing"; elsewhere: { agent_id: string; name: string }[] };

/** What a path a bot wrote is before trying to show it: a file, a folder, or a file that is not on this computer. */
function useStat(agentId: string | undefined, path: string | undefined): Stat | null {
  const [stat, setStat] = useState<Stat | null>(null);
  useEffect(() => {
    if (!agentId || !path) { setStat({ kind: "file" }); return; }
    setStat(null);
    const controller = new AbortController();
    fetch(`/api/agents/${encodeURIComponent(agentId)}/files/stat?path=${encodeURIComponent(path)}`, { signal: controller.signal })
      .then(async response => { if (!response.ok) throw new Error(`HTTP ${response.status}`); setStat(await response.json() as Stat); })
      // Could not ask (an older server, a path it refuses): try to show it as before.
      .catch(() => { if (!controller.signal.aborted) setStat({ kind: "file" }); });
    return () => controller.abort();
  }, [agentId, path]);
  return stat;
}

function parentOf(path: string): string | null {
  const parent = path.replace(/\/+$/, "").slice(0, path.replace(/\/+$/, "").lastIndexOf("/"));
  return /^\/(workspace|home\/box)\/./.test(parent) ? parent : null;
}

function Folder({ path, entries, truncated, onOpenPath }: { path: string; entries: FolderEntry[]; truncated: boolean; onOpenPath?: (path: string) => void }) {
  const parent = parentOf(path);
  return (
    <div className="file-folder">
      <p className="file-cut">這是資料夾，裡面有 {entries.length}{truncated ? "+" : ""} 個項目{onOpenPath ? "，點一下打開" : ""}。</p>
      {parent && onOpenPath ? <button type="button" className="file-folder-up" onClick={() => onOpenPath(parent)}>↑ 上一層</button> : null}
      {entries.length ? <ul>{entries.map(entry => <li key={entry.path} data-dir={entry.dir || undefined}>
        {onOpenPath ? <button type="button" onClick={() => onOpenPath(entry.path)}>{entry.dir ? `${entry.name}/` : entry.name}</button> : <span>{entry.name}</span>}
        {entry.bytes !== null ? <small>{bytes(entry.bytes)}</small> : null}
      </li>)}</ul> : <Status>資料夾是空的。</Status>}
    </div>
  );
}

export default function FilePreview({ file, agentId, onOpenPath }: { file: PreviewFile; agentId?: string; onOpenPath?: (path: string) => void }) {
  const stat = useStat(agentId, file.path);
  // A path the bot named from a teammate's computer, opened there instead.
  const [elsewhere, setElsewhere] = useState<{ agent_id: string; name: string } | null>(null);
  useEffect(() => setElsewhere(null), [file.url]);
  if (elsewhere && file.path) return (
    <div className="file-elsewhere">
      <p className="file-cut">這是 {elsewhere.name} 電腦上的檔案。</p>
      <FileBody file={{ ...file, url: contentUrl(elsewhere.agent_id, file.path) }} agentId={elsewhere.agent_id} />
    </div>
  );
  if (!stat) return <Status>讀取中…</Status>;
  if (stat.kind === "folder" && file.path) return <Folder path={file.path} entries={stat.entries} truncated={stat.truncated} onOpenPath={onOpenPath} />;
  if (stat.kind === "missing") return (
    <div className="file-fallback">
      <strong>{stat.elsewhere.length ? "這個檔案不在這台電腦上" : "找不到這個檔案"}</strong>
      <small>{stat.elsewhere.length ? `它在 ${stat.elsewhere.map(other => other.name).join("、")} 的電腦上；Bot 提到的是隊友那邊的路徑。` : "可能已經被移動、改名或刪除；也可能 Bot 寫錯了路徑。可以在檔案列表裡找找看，或請 Bot 重新給一次路徑。"}</small>
      {stat.elsewhere.map(other => <button type="button" key={other.agent_id} onClick={() => setElsewhere(other)}>打開 {other.name} 那份</button>)}
    </div>
  );
  return <FileBody file={file} agentId={agentId} onOpenPath={onOpenPath} />;
}

function FileBody({ file, agentId, onOpenPath }: { file: PreviewFile; agentId?: string; onOpenPath?: (path: string) => void }) {
  const { kind, language } = useMemo(() => classify(file), [file]);
  const [source, setSource] = useState(false);
  useEffect(() => setSource(false), [file.url]);
  const textual = ["markdown", "code", "csv", "json", "notebook", "html", "sniff"].includes(kind) || (kind === "svg" && source);
  const text = useText(file.url, textual);
  const resolveUrl = useMemo(() => {
    const resolve = (src: string) => (agentId ? toContentUrl(agentId, src, file.path, file.path ? [file.path] : []) : undefined);
    return resolve;
  }, [agentId, file.path]);

  let body: ReactNode;
  if (kind === "image" || kind === "video" || kind === "audio" || (kind === "svg" && !source)) body = <Media kind={kind} url={file.url} file={file} />;
  else if (kind === "pdf") body = <Pdf url={file.url} title={file.title} />;
  else if (kind === "office") body = <Office agentId={agentId} file={file} />;
  else if (kind === "archive") body = <Archive agentId={agentId} file={file} />;
  else if (kind === "font") body = <FontSample url={file.url} title={file.title} />;
  else if (text.status === "loading") body = <Status>讀取中…</Status>;
  else if (text.status === "error") body = <Status>無法讀取這個檔案，請重新整理後再試。</Status>;
  else if (text.status === "binary" || !text.loaded) body = <Fallback file={file} />;
  else {
    const { text: value } = text.loaded;
    if (source || kind === "code" || kind === "sniff") body = <Code text={value} language={kind === "html" || kind === "svg" ? "xml" : kind === "json" || kind === "notebook" ? "json" : kind === "markdown" ? "markdown" : language} />;
    else if (kind === "markdown") body = <MarkdownBody content={value} agentId={agentId} resolveUrl={resolveUrl} onOpenPath={onOpenPath} />;
    else if (kind === "csv") body = <Table text={value} separator={/\.tsv$/i.test(file.title) || (!value.split("\n", 1)[0].includes(",") && value.includes("\t")) ? "\t" : ","} />;
    else if (kind === "json") {
      let pretty: string | null = null;
      try { pretty = text.loaded.truncated ? null : JSON.stringify(JSON.parse(value), null, 2); } catch { pretty = null; }
      body = <Code text={pretty ?? value} language="json" />;
    } else if (kind === "notebook") body = <Notebook text={value} agentId={agentId} resolveUrl={resolveUrl} />;
    else body = <HtmlPage html={value} resolve={resolveUrl} title={file.title} />;
  }

  const framed = (kind === "html" && !source) || kind === "pdf" || kind === "office";
  return (
    <div className={`file-preview${framed ? " is-framed" : ""}`} data-kind={kind}>
      {HAS_SOURCE.includes(kind) ? (
        <div className="file-modes" role="tablist" aria-label="顯示方式">
          <button type="button" role="tab" aria-selected={!source} onClick={() => setSource(false)}>預覽</button>
          <button type="button" role="tab" aria-selected={source} onClick={() => setSource(true)}>原始內容</button>
        </div>
      ) : null}
      {body}
      {textual && text.status === "ready" ? <Cut loaded={text.loaded} /> : null}
    </div>
  );
}
