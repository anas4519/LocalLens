// Date/number formatting shared by every screen. All times are the Mac's local time zone.

const pad = (n: number) => n.toString().padStart(2, "0");

/** "Tue, 10 Nov 2026, 7:55 PM": the date + time every session shows, named or not. */
export const fmtWhen = (ms: number) =>
  new Date(ms).toLocaleString([], {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });

/** Frame time with seconds, e.g. "19:58:32" or "7:58:32 PM" depending on the Mac's locale. */
export const fmtTime = (ts: number) =>
  new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

/** Frame time without seconds, for thumbnail labels. */
export const fmtShortTime = (ts: number) => new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

/** Stopwatch style, always HH:MM:SS: the live session timer. */
export function fmtClock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${pad(Math.floor(s / 3600))}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
}

/** Human duration: "45s", "42m 10s", "1h 14m". */
export function fmtSpan(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${pad(s % 60)}s`;
  return `${Math.floor(s / 3600)}h ${pad(Math.floor((s % 3600) / 60))}m`;
}

export const fmtCount = (n: number) => n.toLocaleString();
export const plural = (n: number, word: string) => `${fmtCount(n)} ${word}${n === 1 ? "" : "s"}`;

/** Local calendar day, used to group sessions under Today / Yesterday / a date. */
export const dayKey = (ms: number) => {
  const d = new Date(ms);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
};

export function dayLabel(ms: number, now = Date.now()): string {
  if (dayKey(ms) === dayKey(now)) return "Today";
  const y = new Date(now);
  y.setDate(y.getDate() - 1);
  if (dayKey(ms) === dayKey(y.getTime())) return "Yesterday";
  return new Date(ms).toLocaleDateString([], { weekday: "short", day: "numeric", month: "short", year: "numeric" });
}
