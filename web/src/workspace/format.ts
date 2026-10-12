const startOfDay = (value: Date) => new Date(value.getFullYear(), value.getMonth(), value.getDate()).getTime();

export function listTime(iso?: string | null, now = new Date()) {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const days = Math.round((startOfDay(now) - startOfDay(date)) / 86400000);
  if (days === 0) return new Intl.DateTimeFormat("zh-TW", { hour: "2-digit", minute: "2-digit", hour12: false }).format(date);
  if (days === 1) return "昨天";
  if (days > 1 && days < 7) return new Intl.DateTimeFormat("zh-TW", { weekday: "short" }).format(date);
  if (date.getFullYear() === now.getFullYear()) return `${date.getMonth() + 1}/${date.getDate()}`;
  return `${date.getFullYear()}/${date.getMonth() + 1}/${date.getDate()}`;
}

export function clock(iso?: string) {
  if (!iso) return "";
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? "" : new Intl.DateTimeFormat("zh-TW", { hour: "2-digit", minute: "2-digit", hour12: false }).format(date);
}

export function dayLabel(iso?: string, now = new Date()) {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const days = Math.round((startOfDay(now) - startOfDay(date)) / 86400000);
  if (days === 0) return "今天";
  if (days === 1) return "昨天";
  return new Intl.DateTimeFormat("zh-TW", { year: date.getFullYear() === now.getFullYear() ? undefined : "numeric", month: "long", day: "numeric", weekday: "short" }).format(date);
}

export function sameDay(a?: string, b?: string) {
  if (!a || !b) return false;
  return startOfDay(new Date(a)) === startOfDay(new Date(b));
}

export function duration(seconds: number) {
  if (seconds < 60) return `${seconds} 秒`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} 分 ${seconds % 60} 秒`;
  return `${Math.floor(minutes / 60)} 小時 ${minutes % 60} 分`;
}

export function bytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(value < 10240 ? 1 : 0)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
