const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function bytes(n: number | null | undefined, digits = 1): string {
  if (n == null || !isFinite(n) || n < 0) return "—";
  if (n < 1024) return `${n} B`;
  let i = 0;
  let v = n;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 ? 0 : digits)} ${UNITS[i]}`;
}

export function speed(n: number): string {
  if (!n || n < 1) return "—";
  return `${bytes(n)}/s`;
}

export function eta(sec: number | null | undefined): string {
  if (sec == null || !isFinite(sec) || sec < 0) return "—";
  if (sec < 1) return "<1s";
  const s = Math.round(sec);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  if (h > 99) return "∞";
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m ${r.toString().padStart(2, "0")}s`;
  return `${r}s`;
}

export function percent(done: number, total: number | null): number {
  if (!total) return 0;
  return Math.min(100, (done / total) * 100);
}

export function relTime(ms: number, lang: string): string {
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  const d = (ms - Date.now()) / 1000;
  const a = Math.abs(d);
  if (a < 60) return rtf.format(Math.round(d), "second");
  if (a < 3600) return rtf.format(Math.round(d / 60), "minute");
  if (a < 86400) return rtf.format(Math.round(d / 3600), "hour");
  if (a < 86400 * 30) return rtf.format(Math.round(d / 86400), "day");
  return new Date(ms).toLocaleDateString(lang);
}

export function host(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, "");
  } catch {
    return url;
  }
}

export function ext(name: string): string {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1).toUpperCase().slice(0, 4) : "";
}

/** Parse "5 MB", "500k", "2m" → bytes/s. Empty or 0 → 0 (unlimited). */
export function parseRate(v: string): number {
  const m = v.trim().toLowerCase().match(/^(\d+(?:[.,]\d+)?)\s*(k|kb|m|mb|g|gb)?/);
  if (!m) return 0;
  const n = parseFloat(m[1].replace(",", "."));
  const unit = m[2]?.[0];
  const mul = unit === "g" ? 1024 ** 3 : unit === "m" ? 1024 ** 2 : unit === "k" ? 1024 : 1024 ** 2;
  return Math.round(n * mul);
}
