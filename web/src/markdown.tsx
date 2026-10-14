import { Component, lazy, memo, Suspense, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Components } from "react-markdown";
import { useI18n } from "./i18n";

const MarkdownEngine = lazy(() => import("./markdown-engine"));
const FilePreview = lazy(() => import("./file-preview"));

class MarkdownLoadBoundary extends Component<{ children: ReactNode; fallback: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() { return this.state.failed ? this.props.fallback : this.props.children; }
}

const FILE_LINK = "#file=";
const BOX_PATH = /\/(?:workspace|home\/box)\/[^\s<>"'`|]+/gu;

function trimPath(path: string): string {
  return path.replace(/[)\]}>.,;:!?，。；、）】》」』*]+$/u, "");
}

// Bots write paths straight into Chinese sentences: "已寫入 /workspace/a.md，內容…", "poc.py（純標準庫）、probe.py".
// These never belong to a file name; a full-width bracket or more Chinese right after an extension does not either.
const SENTENCE = /[，。；、：！？「」『』《》【】]/u;
const ENDS_WITH_EXTENSION = /\.[A-Za-z][A-Za-z0-9]{0,7}$/;

/** The path a bot meant, without the sentence that runs on after it (file names may still hold Chinese and （）). */
function cutPath(raw: string): string {
  for (let i = 1; i < raw.length; i++) {
    const char = raw[i];
    if (SENTENCE.test(char)) return raw.slice(0, i);
    if (!/[A-Za-z0-9._~\/-]/.test(char) && ENDS_WITH_EXTENSION.test(raw.slice(0, i))) return raw.slice(0, i);
  }
  return raw;
}

function isPathChar(value: string): boolean {
  return /[A-Za-z0-9._~/-]/.test(value);
}

/** Absolute box file. Rejects `..`, the directory itself, and host paths. */
export function normalizeChatPath(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed.startsWith("/") || trimmed.includes("\\") || trimmed.includes("\0")) return null;
  const parts: string[] = [];
  for (const part of trimmed.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") return null;
    parts.push(part);
  }
  if (parts.length < 2) return null;
  const clean = `/${parts.join("/")}`;
  if (clean.startsWith("/workspace/") && clean !== "/workspace") return clean;
  if (clean.startsWith("/home/box/") && clean !== "/home/box") return clean;
  return null;
}

function joinUnder(baseFile: string, rel: string): string | null {
  if (!baseFile.startsWith("/") || !rel || rel.startsWith("/") || rel.includes("\\") || rel.includes("\0")) {
    return null;
  }
  const parts = baseFile.slice(0, baseFile.lastIndexOf("/")).split("/");
  for (const part of rel.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (parts.length <= 1) return null;
      parts.pop();
      continue;
    }
    parts.push(part);
  }
  const joined = parts.join("/");
  return joined.startsWith("/") ? joined : null;
}

export function contentUrl(agentId: string, path: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/files/content?path=${encodeURIComponent(path)}`;
}

export function downloadUrl(url: string): string {
  return `${url}${url.includes("?") ? "&" : "?"}download=1`;
}

/** Map a markdown URL onto the file API, or leave ordinary web links alone. */
export function toContentUrl(
  agentId: string,
  src: string,
  baseFile: string | undefined,
  known: readonly string[],
): string | undefined {
  const trimmed = src.trim();
  if (!trimmed || /^(https?:|mailto:)/i.test(trimmed)) return undefined;
  const direct = normalizeChatPath(trimmed);
  if (direct) return contentUrl(agentId, direct);
  if (known.includes(trimmed)) return contentUrl(agentId, trimmed);
  if (!baseFile) return undefined;
  const joined = joinUnder(baseFile, trimmed);
  if (!joined) return undefined;
  if (normalizeChatPath(joined) || normalizeChatPath(baseFile) || known.includes(baseFile)) {
    return contentUrl(agentId, joined);
  }
  return undefined;
}

function fileTitle(path: string): string {
  const name = path.slice(path.lastIndexOf("/") + 1);
  return name || path;
}

/** Spans that already are links — `[label](url)`, `![alt](src)`, `<url>` or a bare web URL — and must not be re-linked. */
const LINKED = /!?\[[^\]\n]*\]\([^)\s]*\)|<[a-z]+:[^>\s]*>|https?:\/\/[^\s<>)\]]+/gi;

function linkifyText(text: string, known: readonly string[]): string {
  type Hit = { start: number; end: number; path: string };
  const hits: Hit[] = [];
  const linked = [...text.matchAll(LINKED)].map(match => [match.index!, match.index! + match[0].length] as const);
  const insideLink = (start: number, end: number) => linked.some(([from, to]) => start < to && end > from);
  for (const match of text.matchAll(new RegExp(BOX_PATH.source, BOX_PATH.flags))) {
    if (match.index === undefined) continue;
    const trimmed = trimPath(cutPath(match[0]));
    // "/workspace/from/水哥哥/…/poc.py": the bot left part of the path out, so it names no real file.
    if (/…|\.\.\./.test(trimmed)) continue;
    const path = normalizeChatPath(trimmed);
    if (!path) continue;
    const before = text.slice(Math.max(0, match.index - 2), match.index);
    if (before.endsWith("(")) continue;
    hits.push({ start: match.index, end: match.index + trimmed.length, path });
  }
  const sorted = [...known].filter((path) => path && !normalizeChatPath(path)).sort((a, b) => b.length - a.length);
  for (const path of sorted) {
    let from = 0;
    while (from < text.length) {
      const start = text.indexOf(path, from);
      if (start < 0) break;
      const before = text.slice(Math.max(0, start - 2), start);
      const after = text[start + path.length] ?? "";
      const prev = text[start - 1] ?? "";
      if (!before.endsWith("(") && !isPathChar(prev) && !isPathChar(after)) {
        hits.push({ start, end: start + path.length, path });
      }
      from = start + path.length;
    }
  }
  // An attachment's own link names its path inside the URL; rewriting it there corrupts the link.
  hits.splice(0, hits.length, ...hits.filter(hit => !insideLink(hit.start, hit.end)));
  hits.sort((a, b) => a.start - b.start);
  let out = "";
  let cursor = 0;
  for (const hit of hits) {
    if (hit.start < cursor) continue;
    const title = fileTitle(hit.path).replace(/[\[\]]/g, "");
    out += text.slice(cursor, hit.start);
    out += `[${title}](${FILE_LINK}${encodeURIComponent(hit.path)})`;
    cursor = hit.end;
  }
  return out + text.slice(cursor);
}

/** Turn bare box and known file paths into links. Leave code, images, and existing links alone. */
export function linkifyChatFiles(content: string, known: readonly string[]): string {
  const fences = content.split(/(```[\s\S]*?```)/g);
  return fences
    .map((part, index) => {
      if (index % 2 === 1) return part;
      const codes = part.split(/(`[^`]*`)/g);
      return codes.map((chunk, codeIndex) => (codeIndex % 2 === 1 ? chunk : linkifyText(chunk, known))).join("");
    })
    .join("");
}

export function previewFromPath(
  agentId: string,
  path: string,
  known?: readonly { path: string; title: string; media_type?: string }[],
): PreviewFile {
  const hit = known?.find((file) => file.path === path);
  return {
    title: hit?.title || fileTitle(path),
    path,
    url: contentUrl(agentId, path),
    mediaType: hit?.media_type,
  };
}

function filePathFromHref(href: string): string | null {
  const marker = href.indexOf(FILE_LINK);
  if (marker < 0) return null;
  try {
    return decodeURIComponent(href.slice(marker + FILE_LINK.length));
  } catch {
    return null;
  }
}

function nodeText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(nodeText).join("");
  return "";
}

/** Bare URLs and links whose label is the URL itself show the site name. */
export function linkLabel(href: string, children: ReactNode): ReactNode {
  const text = nodeText(children).trim();
  if (!text) return children;
  const bare = /^https?:\/\//i.test(text) || text.replace(/\/$/, "") === href.replace(/\/$/, "");
  if (!bare) return children;
  try {
    return new URL(href).hostname.replace(/^www\./, "") || text;
  } catch {
    return children;
  }
}

function supportedUrl(value: string | undefined, protocols: readonly string[]): string | undefined {
  const url = (value ?? "").trim();
  if (!url) return undefined;
  if (url.startsWith("/") && !url.startsWith("//")) {
    if (normalizeChatPath(url)) return undefined;
    return url;
  }
  try {
    const protocol = new URL(url, window.location.origin).protocol.toLowerCase();
    return protocols.includes(protocol) ? url : undefined;
  } catch {
    return undefined;
  }
}

export type PreviewFile = {
  title: string;
  url: string;
  path?: string;
  mediaType?: string;
};

function MarkdownTable({ children }: { children?: ReactNode }) {
  const { t } = useI18n();
  return (
    <div className="md-table-wrap" role="region" aria-label={t.dataTable} tabIndex={0}>
      <table>{children}</table>
    </div>
  );
}

function MarkdownTableCell({
  tag,
  node,
  children,
}: {
  tag: "th" | "td";
  node?: { properties?: Record<string, unknown> };
  children?: ReactNode;
}) {
  const Tag = tag;
  const align = node?.properties?.align;
  const textAlign = align === "left" || align === "center" || align === "right" ? align : undefined;
  return (
    <Tag scope={tag === "th" ? "col" : undefined} style={textAlign ? { textAlign } : undefined}>
      {children}
    </Tag>
  );
}

function MarkdownImage({
  src,
  alt,
  openPath,
  onOpen,
}: {
  src?: string;
  alt?: string;
  openPath?: string;
  onOpen?: (path: string) => void;
}) {
  const { t } = useI18n();
  const [failed, setFailed] = useState(false);
  const [ready, setReady] = useState(false);
  const imageRef = useRef<HTMLImageElement>(null);
  const source = (src ?? "").trim();
  useEffect(() => {
    setFailed(false);
    setReady(false);
    const image = imageRef.current;
    if (image?.complete && image.naturalWidth > 0) setReady(true);
  }, [source]);
  const safeSrc = supportedUrl(source, ["http:", "https:"]);
  if (!safeSrc || failed) {
    const label = alt?.trim() || t.imageUnavailable;
    return (
      <span className="md-image-fallback" role="img" aria-label={label}>
        {label}
      </span>
    );
  }
  const clickable = Boolean(openPath && onOpen);
  return (
    <img
      ref={imageRef}
      className={`md-image${ready ? " is-ready" : ""}`}
      src={safeSrc}
      alt={alt ?? ""}
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      role={clickable ? "button" : undefined}
      tabIndex={clickable ? 0 : undefined}
      onLoad={() => setReady(true)}
      onError={() => setFailed(true)}
      onClick={clickable ? () => onOpen!(openPath!) : undefined}
      onKeyDown={
        clickable
          ? (event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onOpen!(openPath!);
              }
            }
          : undefined
      }
    />
  );
}

const AUDIO_FILE = /\.(mp3|m4a|aac|wav|ogg|oga|opus|flac|webm)$/i;

export function FileChip({ file, onOpen }: { file: PreviewFile; onOpen: () => void }) {
  const { t } = useI18n();
  // A voice memo or recording plays right in the conversation (Grok's "Play voice memo").
  if (AUDIO_FILE.test(file.path || file.title)) {
    return (
      <span className="chat-file chat-audio">
        <span className="chat-file-name" title={file.path || file.title}>{file.title}</span>
        <audio controls preload="metadata" src={file.url} aria-label={`播放 ${file.title}`} />
        <a className="file-download" href={downloadUrl(file.url)} download aria-label={`${t.fileDownload} ${file.title}`}>{t.fileDownload}</a>
      </span>
    );
  }
  return (
    <span className="chat-file">
      <span className="chat-file-name" title={file.path || file.title}>{file.title}</span>
      <button type="button" className="file-open" aria-label={`${t.fileOpen} ${file.title}`} onClick={onOpen}>
        {t.fileOpen}
      </button>
      <a className="file-download" href={downloadUrl(file.url)} download aria-label={`${t.fileDownload} ${file.title}`}>
        {t.fileDownload}
      </a>
    </span>
  );
}

/** The bot's own file API link (what an attachment is sent as) → its /workspace path. */
function workspacePathFromApi(href: string, agentId: string): string | null {
  try {
    const url = new URL(href, window.location.origin);
    if (url.origin !== window.location.origin || url.pathname !== `/api/agents/${agentId}/files/content`) return null;
    const path = url.searchParams.get("path") || "";
    return normalizeChatPath(path.startsWith("/") ? path : `/workspace/${path}`);
  } catch {
    return null;
  }
}

/** An uploaded picture shown in its message; attachments expire, so a missing one says so instead of a broken image. */
function AttachedImage({ file, onOpen }: { file: PreviewFile; onOpen: () => void }) {
  const [gone, setGone] = useState(false);
  if (gone) return <span className="md-attachment-gone">{file.title}（附件已不存在）</span>;
  return (
    <button type="button" className="md-attached-image" aria-label={`開啟 ${file.title}`} onClick={onOpen}>
      <img src={file.url} alt={file.title} loading="lazy" decoding="async" onError={() => setGone(true)} />
    </button>
  );
}

const IMAGE_FILE = /\.(png|jpe?g|webp|gif)$/i;

function MarkdownLink({
  href,
  children,
  agentId,
  onOpenPath,
}: {
  href?: string;
  children?: ReactNode;
  agentId?: string;
  onOpenPath?: (path: string) => void;
}) {
  const linked = href ? filePathFromHref(href) : null;
  if (linked && agentId && onOpenPath) {
    return <FileChip file={previewFromPath(agentId, linked)} onOpen={() => onOpenPath(linked)} />;
  }
  // Attachments open inside the app: a home-screen app would send a plain link out to Safari.
  const attached = href && agentId && onOpenPath ? workspacePathFromApi(href, agentId) : null;
  if (attached && agentId && onOpenPath) {
    const file = previewFromPath(agentId, attached);
    if (IMAGE_FILE.test(attached)) return <AttachedImage file={file} onOpen={() => onOpenPath(attached)} />;
    return <FileChip file={file} onOpen={() => onOpenPath(attached)} />;
  }
  const box = href ? normalizeChatPath(href) : null;
  if (box && onOpenPath) {
    return (
      <button type="button" className="md-file-link" onClick={() => onOpenPath(box)}>
        {children}
      </button>
    );
  }
  const url = supportedUrl(href, ["http:", "https:", "mailto:"]);
  if (!url) return <span className="md-unsafe-link">{children}</span>;
  return (
    <a className="md-link" href={url} title={url} target="_blank" rel="noreferrer noopener">
      {linkLabel(url, children)}
    </a>
  );
}

function useMarkdownComponents(
  agentId: string | undefined,
  resolveUrl: ((src: string) => string | undefined) | undefined,
  onOpenPath: ((path: string) => void) | undefined,
): Components {
  return useMemo(
    () => ({
      img: ({ src, alt }) => {
        const original = (src ?? "").trim();
        return (
          <MarkdownImage
            src={resolveUrl?.(original) ?? original}
            alt={alt}
            openPath={normalizeChatPath(original) || undefined}
            onOpen={onOpenPath}
          />
        );
      },
      a: ({ href, children }) => (
        <MarkdownLink href={href} agentId={agentId} onOpenPath={onOpenPath}>
          {children}
        </MarkdownLink>
      ),
      table: MarkdownTable,
      th: ({ node, children }) => (
        <MarkdownTableCell tag="th" node={node}>
          {children}
        </MarkdownTableCell>
      ),
      td: ({ node, children }) => (
        <MarkdownTableCell tag="td" node={node}>
          {children}
        </MarkdownTableCell>
      ),
    }),
    [agentId, resolveUrl, onOpenPath],
  );
}

export const MarkdownBody = memo(function MarkdownBody({
  content,
  agentId,
  resolveUrl,
  onOpenPath,
}: {
  content: string;
  agentId?: string;
  resolveUrl?: (src: string) => string | undefined;
  onOpenPath?: (path: string) => void;
}) {
  const { t } = useI18n();
  const components = useMarkdownComponents(agentId, resolveUrl, onOpenPath);
  return (
    <div className="message-body md">
      <MarkdownLoadBoundary fallback={<><span className="markdown-loading">{content}</span><small role="status">{t.markdownLoadFailed}</small></>}>
      <Suspense fallback={<span className="markdown-loading">{content}</span>}>
      <MarkdownEngine components={components}>
        {content}
      </MarkdownEngine>
      </Suspense>
      </MarkdownLoadBoundary>
    </div>
  );
});

export const ChatMessageBody = memo(function ChatMessageBody({
  content,
  agentId,
  knownPaths,
  onOpen,
}: {
  content: string;
  agentId: string;
  knownPaths: readonly string[];
  onOpen: (file: PreviewFile) => void;
}) {
  const linked = useMemo(() => linkifyChatFiles(content, knownPaths), [content, knownPaths]);
  const resolveUrl = useCallback(
    (src: string) => toContentUrl(agentId, src, undefined, knownPaths),
    [agentId, knownPaths],
  );
  const onOpenPath = useCallback(
    (path: string) => onOpen(previewFromPath(agentId, path)),
    [agentId, onOpen],
  );
  return <MarkdownBody content={linked} agentId={agentId} resolveUrl={resolveUrl} onOpenPath={onOpenPath} />;
});

export function FileViewer({
  file,
  agentId,
  onClose,
  onOpenPath,
}: {
  file: PreviewFile;
  agentId?: string;
  onClose: () => void;
  onOpenPath?: (file: PreviewFile) => void;
}) {
  const { t } = useI18n();
  const openNested = useCallback(
    (path: string) => {
      if (agentId && onOpenPath) onOpenPath(previewFromPath(agentId, path));
    },
    [agentId, onOpenPath],
  );
  return (
    <section className="file-viewer" aria-label={file.title}>
      <header className="file-viewer-head">
        <strong title={file.path || file.title}>{file.title}</strong>
        <a className="file-download" href={downloadUrl(file.url)} download aria-label={`${t.fileDownload} ${file.title}`}>
          {t.fileDownload}
        </a>
        <button type="button" className="file-viewer-close" aria-label={t.close} onClick={onClose}>
          ×
        </button>
      </header>
      <div className="file-viewer-body">
        <MarkdownLoadBoundary fallback={<p className="file-viewer-status">{t.fileUnavailable}</p>}>
          <Suspense fallback={<p className="file-viewer-status">{t.fileLoading}</p>}>
            <FilePreview key={file.url} file={file} agentId={agentId} onOpenPath={openNested} />
          </Suspense>
        </MarkdownLoadBoundary>
      </div>
    </section>
  );
}
