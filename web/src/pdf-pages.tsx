import { useEffect, useRef, useState } from "react";
import * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

type Doc = Awaited<ReturnType<typeof pdfjs.getDocument>["promise"]>;

/** One page, drawn when it scrolls near the screen, at the width of the pane and the screen's pixel density. */
function Page({ doc, number, width }: { doc: Doc; number: number; width: number }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [visible, setVisible] = useState(number <= 2);
  const [ratio, setRatio] = useState(1.414);
  useEffect(() => {
    const node = canvas.current;
    if (!node || visible) return;
    const observer = new IntersectionObserver(entries => { if (entries.some(entry => entry.isIntersecting)) setVisible(true); }, { rootMargin: "800px 0px" });
    observer.observe(node);
    return () => observer.disconnect();
  }, [visible]);
  useEffect(() => {
    if (!visible || !width) return;
    let task: { cancel: () => void } | null = null;
    let alive = true;
    void doc.getPage(number).then(page => {
      if (!alive || !canvas.current) return;
      const base = page.getViewport({ scale: 1 });
      setRatio(base.height / base.width);
      const scale = width / base.width;
      const density = Math.min(window.devicePixelRatio || 1, 3);
      const viewport = page.getViewport({ scale: scale * density });
      const node = canvas.current;
      node.width = Math.floor(viewport.width); node.height = Math.floor(viewport.height);
      const render = page.render({ canvasContext: node.getContext("2d")!, viewport });
      task = render;
      render.promise.catch(() => {});
    });
    return () => { alive = false; task?.cancel(); };
  }, [doc, number, width, visible]);
  return <canvas ref={canvas} className="pdf-page" style={{ width, height: width * ratio }} aria-label={`第 ${number} 頁`} />;
}

export default function PdfPages({ url }: { url: string }) {
  const box = useRef<HTMLDivElement>(null);
  const [doc, setDoc] = useState<Doc | null>(null);
  const [error, setError] = useState("");
  const [width, setWidth] = useState(0);
  useEffect(() => {
    setDoc(null); setError("");
    const task = pdfjs.getDocument({ url, isEvalSupported: false });
    task.promise.then(setDoc).catch(err => setError(err instanceof Error ? err.message : String(err)));
    return () => { void task.destroy(); };
  }, [url]);
  useEffect(() => {
    const node = box.current;
    if (!node) return;
    const observer = new ResizeObserver(() => setWidth(Math.floor(node.clientWidth)));
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return (
    <div ref={box} className="pdf-pages">
      {error ? <p className="file-viewer-status">這份 PDF 無法顯示：{error}</p> : !doc ? <p className="file-viewer-status">載入 PDF…</p> : (
        <>
          <p className="file-cut">共 {doc.numPages} 頁</p>
          {Array.from({ length: doc.numPages }, (_, i) => <Page key={i} doc={doc} number={i + 1} width={width} />)}
        </>
      )}
    </div>
  );
}
