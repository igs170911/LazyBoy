import { useMemo, useState } from "react";
import type { AgentFile } from "../api";
import { contentUrl, downloadUrl, FileViewer, type PreviewFile } from "../markdown";
import { ChevronLeft, Download, FileText, FolderOpen, Image, RotateCw, Search, Video } from "../ui/icons";
import { bytes, listTime } from "./format";

type Group = { key: string; title: string; files: AgentFile[] };

function groupOf(path: string) {
  if (path.startsWith("uploads/")) return "uploads";
  if (path.startsWith("lessons/")) return "lessons";
  return "outputs";
}

function FileIcon({ file }: { file: AgentFile }) {
  if (file.media_type.startsWith("image/")) return <Image aria-hidden="true" />;
  if (file.media_type.startsWith("video/")) return <Video aria-hidden="true" />;
  return <FileText aria-hidden="true" />;
}

export function toPreview(agentId: string, file: AgentFile): PreviewFile {
  return { title: file.title, path: file.path, url: contentUrl(agentId, file.path), mediaType: file.media_type };
}

export function FilesPane({ agentId, files, preview, onPreview, onReload }: {
  agentId: string;
  files: AgentFile[];
  preview: PreviewFile | null;
  onPreview: (file: PreviewFile | null) => void;
  onReload: () => void;
}) {
  const [query, setQuery] = useState("");
  const [all, setAll] = useState(false);
  const hiddenCount = useMemo(() => files.filter(file => file.important === false).length, [files]);
  const groups = useMemo<Group[]>(() => {
    const needle = query.trim().toLowerCase();
    const hits = files
      // Searching looks through everything; browsing shows only results worth opening.
      .filter(file => needle ? file.path.toLowerCase().includes(needle) : all || file.important !== false)
      .sort((a, b) => b.updated_at.localeCompare(a.updated_at));
    const titles: Record<string, string> = { outputs: "成果", uploads: "你上傳的", lessons: "示範教學" };
    return ["outputs", "uploads", "lessons"]
      .map(key => ({ key, title: titles[key], files: hits.filter(file => groupOf(file.path) === key) }))
      .filter(group => group.files.length);
  }, [files, query, all]);

  if (preview) {
    return (
      <section className="files-pane is-preview" aria-label="檔案預覽">
        <div className="pane-toolbar">
          <button type="button" className="btn small ghost" onClick={() => onPreview(null)}><ChevronLeft />所有檔案</button>
        </div>
        <FileViewer file={preview} agentId={agentId} onClose={() => onPreview(null)} onOpenPath={onPreview} />
      </section>
    );
  }

  return (
    <section className="files-pane" aria-label="檔案">
      <div className="pane-toolbar">
        <label className="search-box">
          <Search aria-hidden="true" />
          <input type="search" value={query} placeholder="搜尋檔案" aria-label="搜尋檔案" onChange={event => setQuery(event.target.value)} />
        </label>
        <button type="button" className="icon-btn" aria-label="重新整理檔案" title="重新整理" onClick={onReload}><RotateCw /></button>
      </div>
      <div className="file-groups">
        {!files.length || (!groups.length && !query.trim()) ? (
          <div className="pane-empty"><FolderOpen /><p>還沒有檔案。Bot 做出來的成果、你上傳的附件都會出現在這裡。</p></div>
        ) : !groups.length ? (
          <div className="pane-empty"><Search /><p>找不到符合「{query}」的檔案</p></div>
        ) : groups.map(group => (
          <section key={group.key} className="file-group">
            <h3>{group.title}<span>{group.files.length}</span></h3>
            <ul>
              {group.files.map(file => {
                const item = toPreview(agentId, file);
                return (
                  <li key={file.path} className="file-row">
                    <button type="button" className="file-row-main" title={file.path} onClick={() => onPreview(item)}>
                      <FileIcon file={file} />
                      <span><strong>{file.title}</strong><small>{[bytes(file.bytes), listTime(file.updated_at)].filter(Boolean).join(" · ")}</small></span>
                    </button>
                    <a className="icon-btn" href={downloadUrl(item.url)} download aria-label={`下載 ${file.title}`} title="下載"><Download /></a>
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
        {hiddenCount && !query.trim() ? (
          <button type="button" className="link-btn files-all" aria-pressed={all} onClick={() => setAll(v => !v)}>
            {all ? "只看重要檔案" : `顯示全部（另有 ${hiddenCount} 個暫存或建置檔）`}
          </button>
        ) : null}
      </div>
    </section>
  );
}
