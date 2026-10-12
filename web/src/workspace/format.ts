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

/** Punycode (RFC 3492) labels such as `xn--rsso55b` back to Unicode, for showing domain names. */
export function unicodeHost(host: string) {
  return host.split(".").map(label => {
    if (!label.toLowerCase().startsWith("xn--")) return label;
    try {
      const input = label.slice(4), base = 36, out: number[] = [];
      let n = 128, i = 0, bias = 72, pos = 0;
      const dash = input.lastIndexOf("-");
      if (dash > 0) { for (const ch of input.slice(0, dash)) out.push(ch.charCodeAt(0)); pos = dash + 1; }
      const digit = (c: number) => c >= 48 && c <= 57 ? c - 22 : c >= 65 && c <= 90 ? c - 65 : c >= 97 && c <= 122 ? c - 97 : base;
      while (pos < input.length) {
        const old = i;
        for (let w = 1, k = base; ; k += base) {
          const d = digit(input.charCodeAt(pos++));
          if (d >= base || pos > input.length + 1) return label;
          i += d * w;
          const t = k <= bias ? 1 : k >= bias + 26 ? 26 : k - bias;
          if (d < t) break;
          w *= base - t;
        }
        let delta = old === 0 ? Math.floor((i - old) / 700) : (i - old) >> 1;
        delta += Math.floor(delta / (out.length + 1));
        let k = 0;
        for (; delta > 455; k += base) delta = Math.floor(delta / 35);
        bias = Math.floor(k + 36 * delta / (delta + 38));
        n += Math.floor(i / (out.length + 1));
        i %= out.length + 1;
        out.splice(i++, 0, n);
      }
      return String.fromCodePoint(...out);
    } catch { return label; }
  }).join(".");
}
