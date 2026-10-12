import { useRef, useState, type DragEvent, type PointerEvent } from "react";
import { useI18n } from "./i18n";

export const AVATAR_SOURCE_MAX_BYTES = 25 * 1024 * 1024;
export const AVATAR_SOURCE_MAX_DIMENSION = 1024;
export const AVATAR_OUTPUT_SIZE = 256;
export const AVATAR_STAGE_SIZE = 260;
export const AVATAR_MIN_ZOOM = 1;
export const AVATAR_MAX_ZOOM = 5;

export type AvatarImage = { dataUrl: string; width: number; height: number };
export type AvatarCrop = { zoom: number; centerX: number; centerY: number };

export function initialAvatarCrop(width: number, height: number): AvatarCrop {
  return { zoom: AVATAR_MIN_ZOOM, centerX: width / 2, centerY: height / 2 };
}

function clampZoom(value: number): number {
  return Number.isFinite(value) ? Math.min(AVATAR_MAX_ZOOM, Math.max(AVATAR_MIN_ZOOM, value)) : AVATAR_MIN_ZOOM;
}

function visibleSide(source: Pick<AvatarImage, "width" | "height">, zoom: number): number {
  return Math.min(source.width, source.height) / clampZoom(zoom);
}

export function clampAvatarCrop(source: Pick<AvatarImage, "width" | "height">, crop: AvatarCrop): AvatarCrop {
  const zoom = clampZoom(crop.zoom);
  const side = visibleSide(source, zoom) / 2;
  return {
    zoom,
    centerX: Math.min(source.width - side, Math.max(side, crop.centerX)),
    centerY: Math.min(source.height - side, Math.max(side, crop.centerY)),
  };
}

export function panAvatarCrop(source: AvatarImage, crop: AvatarCrop, deltaX: number, deltaY: number): AvatarCrop {
  const zoom = clampZoom(crop.zoom);
  const scale = AVATAR_STAGE_SIZE / Math.min(source.width, source.height) * zoom;
  return clampAvatarCrop(source, {
    zoom,
    centerX: crop.centerX - deltaX / scale,
    centerY: crop.centerY - deltaY / scale,
  });
}

function cropRect(source: AvatarImage, crop: AvatarCrop) {
  const bounded = clampAvatarCrop(source, crop);
  const side = visibleSide(source, bounded.zoom);
  return { x: bounded.centerX - side / 2, y: bounded.centerY - side / 2, width: side, height: side };
}

function loadImage(dataUrl: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => image.naturalWidth > 0 && image.naturalHeight > 0 ? resolve(image) : reject(new Error("That image could not be loaded."));
    image.onerror = () => reject(new Error("That image could not be loaded."));
    image.src = dataUrl;
  });
}

function canvasContext(canvas: HTMLCanvasElement): CanvasRenderingContext2D {
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Could not export the avatar.");
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  return context;
}

export async function normalizeDataUrl(dataUrl: string): Promise<AvatarImage> {
  const image = await loadImage(dataUrl);
  const longest = Math.max(image.naturalWidth, image.naturalHeight);
  if (longest <= AVATAR_SOURCE_MAX_DIMENSION) {
    return { dataUrl, width: image.naturalWidth, height: image.naturalHeight };
  }
  const scale = AVATAR_SOURCE_MAX_DIMENSION / longest;
  const width = Math.max(1, Math.round(image.naturalWidth * scale));
  const height = Math.max(1, Math.round(image.naturalHeight * scale));
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  canvasContext(canvas).drawImage(image, 0, 0, width, height);
  return { dataUrl: canvas.toDataURL("image/png"), width, height };
}

export function readImageFile(file: File): Promise<string> {
  if (file.size > AVATAR_SOURCE_MAX_BYTES) return Promise.reject(new Error("Choose an image smaller than 25 MB."));
  if (!file.type.toLowerCase().startsWith("image/")) return Promise.reject(new Error("Choose an image file."));
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => typeof reader.result === "string" ? resolve(reader.result) : reject(new Error("That file could not be read."));
    reader.onerror = () => reject(new Error("That file could not be read."));
    reader.readAsDataURL(file);
  });
}

export async function transformAvatar(source: AvatarImage, action: "rotate" | "flip"): Promise<AvatarImage> {
  const image = await loadImage(source.dataUrl);
  const canvas = document.createElement("canvas");
  canvas.width = action === "rotate" ? source.height : source.width;
  canvas.height = action === "rotate" ? source.width : source.height;
  const context = canvasContext(canvas);
  if (action === "rotate") { context.translate(canvas.width, 0); context.rotate(Math.PI / 2); }
  else { context.translate(canvas.width, 0); context.scale(-1, 1); }
  context.drawImage(image, 0, 0);
  return { dataUrl: canvas.toDataURL("image/png"), width: canvas.width, height: canvas.height };
}

export async function encodeAvatarPng(source: AvatarImage, crop: AvatarCrop): Promise<Blob> {
  const image = await loadImage(source.dataUrl);
  const canvas = document.createElement("canvas");
  canvas.width = AVATAR_OUTPUT_SIZE;
  canvas.height = AVATAR_OUTPUT_SIZE;
  const rect = cropRect(source, crop);
  canvasContext(canvas).drawImage(image, rect.x, rect.y, rect.width, rect.height, 0, 0, AVATAR_OUTPUT_SIZE, AVATAR_OUTPUT_SIZE);
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => blob ? resolve(blob) : reject(new Error("Could not export the avatar.")), "image/png");
  });
}

export function AvatarUploadPanel({
  source,
  crop,
  fileName,
  busy,
  error,
  onFile,
  onZoom,
  onPan,
  onReset,
  onSave,
  onTransform,
  onCenter,
}: {
  source: AvatarImage | null;
  crop: AvatarCrop | null;
  fileName: string | null;
  busy?: boolean;
  error?: string;
  onFile: (file: File) => void;
  onZoom: (zoom: number) => void;
  onPan: (dx: number, dy: number) => void;
  onReset: () => void;
  onSave: () => void;
  onTransform?: (action: "rotate" | "flip") => void;
  onCenter?: () => void;
}) {
  const { t } = useI18n();
  const pointer = useRef<{ id: number; x: number; y: number } | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const [dragOver, setDragOver] = useState(false);

  const take = (list: FileList | File[] | null) => {
    const file = Array.from(list || []).find((item) => item.type.toLowerCase().startsWith("image/"));
    if (file) onFile(file);
  };

  const onDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setDragOver(false);
    take(event.dataTransfer.files);
  };

  const imageStyle = source && crop ? (() => {
    const scale = AVATAR_STAGE_SIZE / Math.min(source.width, source.height) * crop.zoom;
    return {
      width: source.width * scale,
      height: source.height * scale,
      transform: `translate(${AVATAR_STAGE_SIZE / 2 - crop.centerX * scale}px, ${AVATAR_STAGE_SIZE / 2 - crop.centerY * scale}px)`,
    };
  })() : undefined;

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    pointer.current = { id: event.pointerId, x: event.clientX, y: event.clientY };
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const current = pointer.current;
    if (!current || current.id !== event.pointerId) return;
    onPan(event.clientX - current.x, event.clientY - current.y);
    current.x = event.clientX;
    current.y = event.clientY;
  };
  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (pointer.current?.id !== event.pointerId) return;
    pointer.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
  };

  if (source && crop) {
    return (
      <div className="avatar-upload">
        <div
          className="avatar-stage"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
          role="img"
          aria-label={t.avatarReposition}
        >
          <img alt="" draggable={false} src={source.dataUrl} style={imageStyle} />
          <span className="avatar-stage-mask" aria-hidden="true" />
        </div>
        <small>{fileName ? `${fileName} · ${source.width}×${source.height}` : t.avatarReposition}</small>
        <div className="avatar-zoom">
          <button type="button" disabled={busy} onClick={() => onZoom(crop.zoom - 0.5)} aria-label="-">−</button>
          <input type="range" min={1} max={5} step={0.01} value={crop.zoom} disabled={busy} onChange={(event) => onZoom(Number.parseFloat(event.currentTarget.value))} aria-label={t.avatarZoom} />
          <button type="button" disabled={busy} onClick={() => onZoom(crop.zoom + 0.5)} aria-label="+">+</button>
        </div>
        <div className="avatar-edit-tools">
          <button type="button" disabled={busy} onClick={() => onTransform?.("rotate")}>旋轉 90°</button>
          <button type="button" disabled={busy} onClick={() => onTransform?.("flip")}>左右翻轉</button>
          <button type="button" disabled={busy} onClick={onCenter}>置中</button>
        </div>
        <div className="pane-actions">
          <button type="button" className="outline" disabled={busy} onClick={onReset}>{t.avatarRestart}</button>
          <button type="button" className="primary" disabled={busy} onClick={onSave}>{busy ? t.saving : t.avatarSet}</button>
        </div>
        {error ? <div className="pane-error">{error}</div> : null}
      </div>
    );
  }

  return (
    <div
      className={`avatar-dropzone ${dragOver ? "over" : ""}`}
      onDragOver={(event) => { event.preventDefault(); setDragOver(true); }}
      onDragLeave={() => setDragOver(false)}
      onDrop={onDrop}
    >
      <strong>{t.avatarDrop}</strong>
      <span>{t.avatarOr}</span>
      <button type="button" className="outline" onClick={() => fileRef.current?.click()}>{t.avatarBrowse}</button>
      <input ref={fileRef} type="file" accept="image/*" hidden onChange={(event) => { take(event.currentTarget.files); event.currentTarget.value = ""; }} />
      {error ? <div className="pane-error">{error}</div> : null}
    </div>
  );
}
