import { convertFileSrc } from "@tauri-apps/api/core";
import { RefObject, useEffect, useMemo, useRef, useState } from "react";
import SessionTitle from "./SessionTitle";
import { FrameRow, MAX_SESSION_NAME, OCR_FAILED, OCR_PENDING, Session, listFrames } from "./api";
import { dayKey, dayLabel, fmtClock, fmtShortTime, fmtSpan, fmtWhen, plural } from "./format";
import { Icon, RecDot, Spinner, isTyping } from "./ui";

/* ---------- Thumbnail strip data ---------- */

type Thumb = Pick<FrameRow, "id" | "ts" | "image_path">;

interface Preview {
  thumbs: Thumb[]; // up to 4, spread evenly across the session
  pending: number; // frames still waiting for OCR
  failed: number;
}

const THUMBS = 4;

function summarize(frames: FrameRow[]): Preview {
  const k = Math.min(THUMBS, frames.length);
  const picks = Array.from({ length: k }, (_, i) => frames[k === 1 ? 0 : Math.round((i * (frames.length - 1)) / (k - 1))]);
  return {
    // Keep only what the strip needs; the OCR text can be large and isn't shown here.
    thumbs: picks.map(({ id, ts, image_path }) => ({ id, ts, image_path })),
    pending: frames.filter((f) => f.ocr_status === OCR_PENDING).length,
    failed: frames.filter((f) => f.ocr_status === OCR_FAILED).length,
  };
}

/** Survives re-renders and trips into the gallery, so the list doesn't refetch every time. */
const previewCache = new Map<number, { frameCount: number; preview: Preview }>();

/**
 * Loads a session's thumbnails the first time its card scrolls into view (list_frames is per
 * session, so we don't fetch every session up front). Re-polls while that session's OCR is behind.
 */
function usePreview(session: Session, ref: RefObject<HTMLElement>): Preview | null {
  const fresh = (c?: { frameCount: number }) => c && c.frameCount === session.frame_count;
  const [preview, setPreview] = useState<Preview | null>(() => {
    const c = previewCache.get(session.id);
    return fresh(c) ? c!.preview : null;
  });

  useEffect(() => {
    const c = previewCache.get(session.id);
    if (fresh(c) && c!.preview.pending === 0) {
      setPreview(c!.preview);
      return;
    }
    const el = ref.current;
    if (!el) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const load = () =>
      listFrames(session.id)
        .then((frames) => {
          if (cancelled) return;
          const p = summarize(frames);
          previewCache.set(session.id, { frameCount: frames.length, preview: p });
          setPreview(p);
          if (p.pending > 0) timer = setTimeout(load, 3000);
        })
        .catch(() => {}); // the card still works without thumbnails
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          io.disconnect();
          load();
        }
      },
      { rootMargin: "200px" },
    );
    io.observe(el);
    return () => {
      cancelled = true;
      io.disconnect();
      clearTimeout(timer);
    };
  }, [session.id, session.frame_count, ref]);

  return preview;
}

/* ---------- One past session ---------- */

function SessionCard({
  session,
  selected,
  editSignal,
  onOpen,
  onRenamed,
}: {
  session: Session;
  selected: boolean;
  editSignal?: number;
  onOpen: () => void;
  onRenamed: (s: Session) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const preview = usePreview(session, ref);
  const duration = (session.ended_at ?? session.started_at) - session.started_at;

  return (
    <div
      ref={ref}
      data-session-id={session.id}
      className={`group relative rounded-xl bg-surface-container p-space-md shadow-md hover:bg-surface-container-high transition-all cursor-pointer ${
        selected ? "bg-surface-container-high ring-1 ring-primary/50" : ""
      }`}
      onClick={onOpen}
    >
      <div className="flex flex-col gap-space-md">
        <div className="flex items-start justify-between gap-space-md">
          <div className="flex items-center gap-space-md min-w-0">
            <div className="w-8 h-8 rounded-lg bg-surface-container-high group-hover:bg-surface-variant flex items-center justify-center shrink-0">
              <Icon
                name={session.name ? "auto_stories" : "schedule"}
                className={`text-[18px] ${session.name ? "text-primary" : "text-outline"}`}
              />
            </div>
            <div className="flex flex-col min-w-0">
              <SessionTitle session={session} onRenamed={onRenamed} variant="card" editSignal={editSignal} />
              <div className="flex items-center gap-space-xs font-body-sm text-body-sm text-outline">
                {session.name && (
                  <>
                    <span>{fmtWhen(session.started_at)}</span>
                    <span>·</span>
                  </>
                )}
                <span className="text-on-surface-variant font-medium">{fmtSpan(duration)}</span>
                <span>·</span>
                <span className="font-label-sm text-label-sm">{plural(session.frame_count, "frame")}</span>
              </div>
            </div>
          </div>
          {/* Quick Action Pills */}
          <div className="flex items-center gap-space-xs opacity-75 group-hover:opacity-100 shrink-0">
            {preview && session.frame_count > 0 && (
              <div className="px-2 py-1 rounded bg-surface-container-lowest font-label-sm text-label-sm text-outline flex items-center gap-1">
                {preview.pending > 0 ? (
                  <>
                    <Spinner className="h-3 w-3 text-secondary" />
                    <span>OCR {preview.pending} pending</span>
                  </>
                ) : (
                  <>
                    <Icon name="search_check" className="text-[12px] text-primary" />
                    <span>Full OCR Ready</span>
                  </>
                )}
              </div>
            )}
            <div className="px-2 py-1 rounded bg-surface-container-lowest font-shortcut-key text-shortcut-key text-outline">
              ↵ View
            </div>
          </div>
        </div>

        {/* Thumbnail Reel Preview (up to 4 frames) */}
        {session.frame_count > 0 && (
          <div className="grid grid-cols-2 md:grid-cols-4 gap-space-sm pt-space-xs">
            {preview
              ? preview.thumbs.map((t) =>
                  t.image_path ? (
                    <div
                      key={t.id}
                      className="relative group/thumb rounded-lg overflow-hidden bg-surface-container-lowest aspect-video shadow-inner"
                    >
                      <img
                        className="w-full h-full object-cover transition-transform duration-300 group-hover/thumb:scale-105"
                        src={convertFileSrc(t.image_path)}
                        loading="lazy"
                        decoding="async"
                        alt=""
                      />
                      <div className="absolute bottom-1.5 left-1.5 px-1.5 py-0.5 rounded bg-surface-container-lowest/80 backdrop-blur-md font-label-sm text-label-sm text-on-surface">
                        {fmtShortTime(t.ts)}
                      </div>
                    </div>
                  ) : (
                    // Pruned Image Placeholder Item
                    <div
                      key={t.id}
                      className="relative rounded-lg overflow-hidden bg-surface-container-low aspect-video flex flex-col items-center justify-center text-center p-space-sm shadow-inner"
                    >
                      <Icon name="filter_drama" className="text-[20px] text-outline" />
                      <span className="font-label-sm text-label-sm text-on-surface font-medium mt-1">Image expired</span>
                      <span className="font-label-sm text-outline text-[9px] mt-0.5">
                        Text kept · {fmtShortTime(t.ts)}
                      </span>
                    </div>
                  ),
                )
              : Array.from({ length: Math.min(THUMBS, session.frame_count) }, (_, i) => (
                  <div key={i} className="rounded-lg bg-surface-container-lowest aspect-video animate-pulse" />
                ))}
            {/* Inactive Slot Placeholder */}
            {preview && preview.thumbs.length < THUMBS && (
              <div className="relative rounded-lg bg-surface-container-lowest/40 aspect-video hidden md:flex items-center justify-center">
                <span className="font-label-sm text-label-sm text-outline-variant">End of session</span>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

/* ---------- The screen ---------- */

interface Props {
  sessions: Session[];
  active: Session | null;
  now: number;
  ocrPending: number;
  hasPermission: boolean;
  captureError: string | null;
  error: string | null;
  busy: boolean;
  newName: string;
  onNewName: (v: string) => void;
  onToggle: () => void;
  onOpen: (s: Session) => void;
  onRenamed: (s: Session) => void;
  version: string | null;
}

export default function Sessions(p: Props) {
  const past = useMemo(() => p.sessions.filter((s) => s.ended_at !== null), [p.sessions]);

  // Group by local day, keeping newest-first order from list_sessions.
  const groups = useMemo(() => {
    const out: { key: string; label: string; items: Session[] }[] = [];
    for (const s of past) {
      const key = dayKey(s.started_at);
      const last = out[out.length - 1];
      if (last && last.key === key) last.items.push(s);
      else out.push({ key, label: dayLabel(s.started_at), items: [s] });
    }
    return out;
  }, [past]);

  // Keyboard: ↑↓ pick a session, ↵ opens it, R renames it.
  const [sel, setSel] = useState(-1);
  const [rename, setRename] = useState<{ id: number; n: number } | null>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e.target) || e.metaKey || e.ctrlKey || e.altKey || past.length === 0) return;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSel((i) => Math.min(past.length - 1, i + 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSel((i) => Math.max(0, i - 1));
      } else if (e.key === "Enter" && past[sel] && !(e.target instanceof HTMLButtonElement)) {
        p.onOpen(past[sel]);
      } else if (e.key.toLowerCase() === "r" && past[sel]) {
        e.preventDefault();
        setRename({ id: past[sel].id, n: Date.now() });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [past, sel, p]);

  useEffect(() => {
    if (!past[sel]) return;
    document.querySelector(`[data-session-id="${past[sel].id}"]`)?.scrollIntoView({ block: "nearest" });
  }, [sel, past]);

  const { active } = p;

  return (
    <div className="flex flex-col w-full">
      <div className="p-space-xl flex flex-col gap-space-xl max-w-7xl mx-auto w-full">
        {/* Top Interactive Recorder Hero / Master Controller */}
        <div className="flex flex-col gap-space-md">
          {!p.hasPermission && (
            <div className="flex items-start gap-space-sm px-space-md py-space-sm rounded-xl bg-error-container/30 shadow-sm">
              <Icon name="screen_record" className="text-[18px] text-error mt-0.5" />
              <div className="flex flex-col gap-1">
                <span className="font-body-md text-body-md font-semibold text-on-surface">
                  Screen Recording permission needed
                </span>
                <span className="font-body-sm text-body-sm text-on-surface-variant">
                  Open System Settings → Privacy &amp; Security → Screen Recording, enable LocalLens (or your terminal in
                  dev mode), then quit and reopen the app.
                </span>
              </div>
            </div>
          )}

          {active ? (
            /* Active Live Recording Glass Banner */
            <div className="relative overflow-hidden rounded-xl bg-surface-container-high/90 shadow-[0_16px_40px_-12px_rgba(0,0,0,0.7)] p-space-lg backdrop-blur-xl">
              {/* Ambient Radial Glows */}
              <div className="pointer-events-none absolute -top-16 -right-16 w-80 h-80 rounded-full bg-error/10 blur-3xl" />
              <div className="pointer-events-none absolute -bottom-16 -left-16 w-80 h-80 rounded-full bg-primary/10 blur-3xl" />
              <div className="relative flex flex-col lg:flex-row lg:items-center justify-between gap-space-lg">
                {/* Session Context & Live Metrics */}
                <div className="flex flex-col gap-space-sm min-w-0 flex-1">
                  <div className="flex items-center gap-space-sm flex-wrap">
                    <div className="flex items-center gap-space-xs px-2 py-0.5 rounded-full bg-surface-container-lowest/80 text-on-surface">
                      <RecDot />
                      <span className="font-label-sm text-label-sm tracking-wider uppercase text-error">LIVE SESSION</span>
                    </div>
                    <span className="text-outline-variant font-label-sm text-label-sm">/</span>
                    <span className="font-label-sm text-label-sm text-on-surface-variant">OCR: APPLE VISION · ON-DEVICE</span>
                    <span className="text-outline-variant font-label-sm text-label-sm">/</span>
                    <div className="flex items-center gap-1 text-primary">
                      <Icon name="memory" className="text-[14px]" />
                      <span className="font-label-sm text-label-sm">SCREENCAPTUREKIT</span>
                    </div>
                  </div>
                  <div className="flex items-baseline gap-space-md flex-wrap pt-0.5">
                    <SessionTitle session={active} onRenamed={p.onRenamed} variant="live" as="h2" />
                    <div className="font-label-md text-label-md px-2 py-0.5 rounded bg-surface-container font-medium text-secondary flex items-center gap-1 shadow-sm">
                      <Icon name="timer" className="text-[13px] text-secondary" />
                      <span>{fmtClock(p.now - active.started_at)}</span>
                    </div>
                  </div>
                  <div className="flex items-center gap-space-lg text-on-surface-variant pt-1 flex-wrap">
                    <div className="flex items-center gap-1.5">
                      <Icon name="photo_library" className="text-[15px] text-outline" />
                      <span className="font-body-sm text-body-sm text-on-surface font-medium">
                        {active.frame_count.toLocaleString()}
                      </span>
                      <span className="font-body-sm text-body-sm text-outline">
                        {active.frame_count === 1 ? "frame" : "frames"} captured
                      </span>
                      <span className="font-label-sm text-label-sm text-outline-variant">
                        (1 frame/2s · duplicates auto-skipped)
                      </span>
                    </div>
                    <div className="flex items-center gap-2">
                      {p.ocrPending > 0 ? (
                        <>
                          <Spinner className="h-3 w-3 text-secondary" />
                          <span className="font-label-sm text-label-sm text-secondary">
                            OCR pending: {p.ocrPending} queued
                          </span>
                        </>
                      ) : (
                        <>
                          <Icon name="check_circle" className="text-[13px] text-secondary" />
                          <span className="font-label-sm text-label-sm text-secondary">OCR up to date</span>
                        </>
                      )}
                    </div>
                  </div>
                  {active.name && (
                    <span className="font-label-sm text-label-sm text-outline">Started {fmtWhen(active.started_at)}</span>
                  )}
                </div>
                {/* Controls Bar */}
                <div className="flex items-center gap-space-sm self-start lg:self-center">
                  <button
                    className="px-space-md py-2 rounded-lg bg-error-container text-on-error-container hover:opacity-90 flex items-center gap-space-xs transition-all shadow-md cursor-pointer disabled:opacity-50"
                    onClick={p.onToggle}
                    disabled={p.busy}
                  >
                    <Icon name="stop_circle" className="text-[16px] text-error font-semibold" />
                    <span className="font-body-md text-body-md font-semibold text-on-error-container">Stop Session</span>
                  </button>
                </div>
              </div>
            </div>
          ) : (
            /* Quick Session Setup: optional name + Start */
            <div className="flex items-center justify-between gap-space-md px-space-md py-space-sm rounded-xl bg-surface-container-low shadow-sm">
              <div className="flex items-center gap-space-sm flex-1 max-w-xl">
                <Icon name="add_circle" className="text-[16px] text-outline" />
                <input
                  className="bg-transparent text-on-surface placeholder:text-outline font-body-md text-body-md focus:outline-none w-full select-text"
                  placeholder="Session name (optional), e.g. Spring Retry deep-dive"
                  type="text"
                  value={p.newName}
                  maxLength={MAX_SESSION_NAME}
                  disabled={p.busy}
                  onChange={(e) => p.onNewName(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && !p.busy && p.onToggle()}
                  aria-label="Session name (optional)"
                />
              </div>
              <div className="flex items-center gap-space-sm">
                <span className="font-label-sm text-label-sm text-outline hidden md:inline">
                  AUTO-INDEX WITH ON-DEVICE VISION
                </span>
                <button
                  className="px-space-md py-1.5 rounded-lg bg-primary text-on-primary hover:bg-primary-container hover:text-on-primary-container flex items-center gap-space-xs transition-colors text-body-sm font-medium disabled:opacity-50"
                  onClick={p.onToggle}
                  disabled={p.busy}
                >
                  <span>Start Session</span>
                  <kbd className="px-1.5 py-0.2 rounded bg-black/20 font-shortcut-key text-shortcut-key">↵</kbd>
                </button>
              </div>
            </div>
          )}

          {p.captureError && (
            <p className="font-body-sm text-body-sm text-error px-space-xs">Capture problem: {p.captureError}</p>
          )}
          {p.error && <p className="font-body-sm text-body-sm text-error px-space-xs">{p.error}</p>}
        </div>

        {/* Grouped Session History */}
        <div className="flex flex-col gap-space-xl">
          {groups.length === 0 && (
            <div className="rounded-xl bg-surface-container p-space-xl shadow-md flex flex-col items-center text-center gap-space-sm">
              <div className="w-10 h-10 rounded-lg bg-surface-container-high flex items-center justify-center">
                <Icon name="history" className="text-[22px] text-primary" />
              </div>
              <span className="font-headline-sm text-headline-sm font-semibold text-on-surface">No sessions yet</span>
              <span className="font-body-md text-body-md text-on-surface-variant max-w-md">
                Start one above. LocalLens takes a screenshot every 2 seconds, reads its text on this Mac, and makes it
                searchable. Nothing leaves your computer.
              </span>
            </div>
          )}
          {groups.map((g, gi) => {
            const total = g.items.reduce((ms, s) => ms + ((s.ended_at ?? s.started_at) - s.started_at), 0);
            const frames = g.items.reduce((n, s) => n + s.frame_count, 0);
            return (
              <div key={g.key} className="flex flex-col gap-space-md">
                {/* Section Header */}
                <div className="flex items-center justify-between px-space-xs">
                  <div className="flex items-center gap-space-sm">
                    <span className="font-headline-sm text-headline-sm font-semibold text-on-surface tracking-tight">
                      {g.label}
                    </span>
                    <span
                      className={`font-label-sm text-label-sm px-2 py-0.5 rounded-full bg-surface-container-high ${
                        gi === 0 ? "text-secondary" : "text-outline"
                      }`}
                    >
                      {plural(g.items.length, "session")}
                    </span>
                  </div>
                  <div className="flex items-center gap-space-md font-label-sm text-label-sm text-outline">
                    <span className="flex items-center gap-1">
                      <Icon name="photo_library" className="text-[14px]" />
                      <span>{plural(frames, "frame")}</span>
                    </span>
                    <span className="text-outline-variant">·</span>
                    <span>{fmtSpan(total)} total recorded</span>
                  </div>
                </div>
                {g.items.map((s) => (
                  <SessionCard
                    key={s.id}
                    session={s}
                    selected={past[sel]?.id === s.id}
                    editSignal={rename?.id === s.id ? rename.n : undefined}
                    onOpen={() => p.onOpen(s)}
                    onRenamed={p.onRenamed}
                  />
                ))}
              </div>
            );
          })}
        </div>

        {/* Keyboard Navigation Footer Bar */}
        {past.length > 0 && (
          <div className="sticky bottom-4 z-20 flex items-center justify-between px-space-md py-2 rounded-xl bg-surface-container-high/90 backdrop-blur-xl shadow-[0_8px_24px_rgba(0,0,0,0.4)] text-on-surface-variant font-body-sm text-body-sm">
            <div className="flex items-center gap-space-lg">
              <div className="flex items-center gap-space-xs">
                <kbd className="px-1.5 py-0.5 rounded bg-surface-container-lowest font-shortcut-key text-shortcut-key text-outline">↑</kbd>
                <kbd className="px-1.5 py-0.5 rounded bg-surface-container-lowest font-shortcut-key text-shortcut-key text-outline">↓</kbd>
                <span className="text-outline">Navigate</span>
              </div>
              <div className="flex items-center gap-space-xs">
                <kbd className="px-1.5 py-0.5 rounded bg-surface-container-lowest font-shortcut-key text-shortcut-key text-outline">↵</kbd>
                <span className="text-outline">Open Gallery</span>
              </div>
              <div className="flex items-center gap-space-xs">
                <kbd className="px-1.5 py-0.5 rounded bg-surface-container-lowest font-shortcut-key text-shortcut-key text-outline">R</kbd>
                <span className="text-outline">Rename</span>
              </div>
            </div>
            <div className="flex items-center gap-space-sm font-label-sm text-label-sm text-outline">
              <span className="flex h-1.5 w-1.5 rounded-full bg-primary" />
              <span>LocalLens{p.version ? ` v${p.version}` : ""}</span>
              <span>·</span>
              <span>Stored only on this Mac</span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
