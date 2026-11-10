import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import SessionTitle from "./SessionTitle";
import { FrameRow, OCR_FAILED, OCR_PENDING, Session, listFrames } from "./api";
import { fmtSpan, fmtTime, fmtWhen, plural } from "./format";
import { Marked, countHits, matchesAll, searchTerms, splitHits } from "./highlight";
import { Icon, Spinner, hasTextSelection } from "./ui";

/** First non-empty OCR line, as a stand-in for the window title the design shows under each card. */
const firstLine = (t: string) => t.split("\n").find((l) => l.trim())?.trim() ?? "";

function ocrLabel(f: FrameRow): { text: string; cls: string } {
  if (!f.image_path) return { text: "Pruned", cls: "text-outline" };
  if (f.ocr_status === OCR_PENDING) return { text: "OCR: Pending", cls: "text-secondary" };
  if (f.ocr_status === OCR_FAILED) return { text: "OCR: Failed", cls: "text-error" };
  return { text: "OCR: Done", cls: "text-outline" };
}

/* ---------- Right-hand OCR panel of the lightbox ---------- */

function OcrPanel({ frame, terms }: { frame: FrameRow; terms: string[] }) {
  const [copied, setCopied] = useState(false);
  const pending = frame.ocr_status === OCR_PENDING;
  const failed = frame.ocr_status === OCR_FAILED;
  const text = frame.ocr_text.trim();
  const parts = useMemo(() => splitHits(frame.ocr_text, terms), [frame.ocr_text, terms]);

  const copy = useCallback(async () => {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(frame.ocr_text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* webview may deny clipboard; the text is still selectable */
    }
  }, [text, frame.ocr_text]);

  // ⌘C with nothing selected copies the whole frame's text; with a selection it's a normal copy.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "c" && !hasTextSelection()) {
        e.preventDefault();
        copy();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [copy]);

  let body: ReactNode;
  if (pending)
    body = (
      <p className="flex items-center gap-2 p-space-xs text-secondary">
        <Spinner className="h-3 w-3" /> Reading text…
      </p>
    );
  else if (failed) body = <p className="p-space-xs text-error">Couldn’t read this frame</p>;
  else if (!text) body = <p className="p-space-xs text-outline">No text found</p>;
  else
    body = (
      <div className="p-space-sm rounded bg-surface-container-lowest font-label-sm text-label-sm text-on-surface whitespace-pre-wrap break-words">
        <Marked parts={parts} markClass="bg-primary/30 text-primary-fixed font-bold px-0.5 rounded" />
      </div>
    );

  const lines = text ? text.split("\n").length : 0;

  return (
    <div className="w-full lg:w-[340px] flex flex-col justify-between bg-surface-container shadow-md min-h-0">
      {/* OCR Metadata Top */}
      <div className="flex flex-col p-space-md gap-space-sm bg-surface-container-low shadow-sm">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-space-xs">
            <Icon name="text_snippet" className="text-[18px] text-primary" />
            <span className="font-headline-sm text-headline-sm font-semibold text-on-surface">Extracted OCR Text</span>
          </div>
          {text ? (
            <button
              className="flex items-center gap-1 px-space-sm py-1 rounded-md bg-primary hover:bg-primary-container text-on-primary transition-colors font-body-sm text-body-sm font-medium shadow-sm"
              title="Copy text to clipboard"
              onClick={copy}
            >
              <Icon name={copied ? "check" : "content_copy"} className="text-[14px]" />
              <span>{copied ? "Copied" : "Copy All"}</span>
              <kbd className="px-1 py-0.2 rounded bg-black/20 font-shortcut-key text-shortcut-key">⌘C</kbd>
            </button>
          ) : null}
        </div>
        <div className="flex items-center justify-between text-outline font-label-sm text-label-sm">
          <span className="flex items-center gap-1 text-primary">
            <Icon name="verified" className="text-[14px]" />
            Read on-device
          </span>
          <span>Apple Vision</span>
        </div>
      </div>
      {/* Scrollable Raw Monospace OCR Stream */}
      <div className="flex-1 p-space-md overflow-y-auto min-h-0 bg-surface-container font-label-sm text-label-sm leading-relaxed text-on-surface-variant flex flex-col gap-space-sm select-text">
        <div className="text-outline uppercase text-[10px] tracking-wider font-semibold">Raw Text</div>
        {body}
      </div>
      {/* Panel Bottom */}
      <div className="p-space-md bg-surface-container-low flex flex-col gap-space-xs">
        <div className="flex items-center justify-between text-outline font-label-sm text-label-sm">
          <span>Text stats</span>
          <span className="text-on-surface-variant">
            {text ? `${plural(text.length, "char")} · ${plural(lines, "line")}` : "—"}
          </span>
        </div>
      </div>
    </div>
  );
}

/* ---------- The screen ---------- */

export default function Gallery({
  session,
  onBack,
  onRenamed,
  initialFrameId = null,
  highlight = "",
  backLabel = "All Sessions",
}: {
  session: Session;
  onBack: () => void;
  onRenamed: (s: Session) => void;
  /** Open the lightbox on this frame once frames load (used by search results). */
  initialFrameId?: number | null;
  /** The search that brought us here; its words get highlighted in the OCR panel. */
  highlight?: string;
  backLabel?: string;
}) {
  const [frames, setFrames] = useState<FrameRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [openId, setOpenId] = useState<number | null>(null); // frame shown in the lightbox
  const [lastId, setLastId] = useState<number | null>(null); // ring stays on it after closing
  const [filter, setFilter] = useState("");
  const [fit, setFit] = useState(true);
  const [dims, setDims] = useState<{ w: number; h: number } | null>(null);
  const jumpTo = useRef(initialFrameId); // consumed once, so later refreshes don't re-open it
  const filterInput = useRef<HTMLInputElement>(null);

  const refreshFrames = useCallback(() => {
    listFrames(session.id).then(setFrames).catch((e) => setError(String(e)));
  }, [session.id]);

  useEffect(() => {
    refreshFrames();
  }, [refreshFrames]);

  useEffect(() => {
    if (!frames || jumpTo.current === null) return;
    const id = jumpTo.current;
    jumpTo.current = null;
    if (frames.some((f) => f.id === id)) open(id);
  }, [frames]);

  // The in-session filter: every word must start a word in the frame's OCR text.
  const filterTerms = useMemo(() => searchTerms(filter), [filter]);
  const visible = useMemo(
    () => (frames && filterTerms.length ? frames.filter((f) => matchesAll(f.ocr_text, filterTerms)) : frames),
    [frames, filterTerms],
  );
  // Highlight the filter if there is one, otherwise the search that opened this gallery.
  const hlTerms = useMemo(
    () => (filterTerms.length ? filterTerms : searchTerms(highlight)),
    [filterTerms, highlight],
  );

  const pending = frames?.filter((f) => f.ocr_status === OCR_PENDING).length ?? 0;

  // Refresh while OCR is still catching up, including the open lightbox.
  useEffect(() => {
    if (pending === 0) return;
    const t = setInterval(refreshFrames, 2000);
    return () => clearInterval(t);
  }, [pending, refreshFrames]);

  const count = visible?.length ?? 0;
  const index = openId === null || !visible ? -1 : visible.findIndex((f) => f.id === openId);
  const current = index >= 0 && visible ? visible[index] : null;

  function open(id: number) {
    setOpenId(id);
    setLastId(id);
    setDims(null);
  }

  const step = useCallback(
    (d: number) => {
      if (!visible || index < 0) return;
      const next = visible[Math.min(count - 1, Math.max(0, index + d))];
      if (next && next.id !== openId) open(next.id);
    },
    [visible, index, count, openId],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "[") {
        e.preventDefault();
        onBack();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setOpenId(null);
        filterInput.current?.focus();
      } else if (current === null) {
        if (e.key === "Escape") onBack();
      } else if (e.key === "Escape") setOpenId(null);
      else if (e.key === "ArrowRight") step(1);
      else if (e.key === "ArrowLeft") step(-1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [current, step, onBack]);

  const duration = (session.ended_at ?? Date.now()) - session.started_at;
  const occurrences = current && hlTerms.length ? countHits(current.ocr_text, hlTerms) : 0;

  return (
    <div className="flex flex-col w-full">
      {/* Top Session Context Header & Controls */}
      <section className="flex flex-col px-space-xl py-space-lg gap-space-md bg-surface-container-low shadow-sm">
        <div className="flex flex-wrap items-center justify-between gap-space-md">
          {/* Breadcrumb & Title Cluster */}
          <div className="flex items-center gap-space-md min-w-0">
            <button
              className="group flex items-center gap-space-xs px-space-sm py-1 rounded-lg bg-surface-container hover:bg-surface-container-high transition-colors text-on-surface-variant hover:text-on-surface shrink-0"
              onClick={onBack}
            >
              <Icon name="arrow_back" className="text-[16px]" />
              <span className="font-body-md text-body-md font-medium">{backLabel}</span>
              <kbd className="ml-1 px-1 py-0.5 rounded bg-surface-container-highest font-shortcut-key text-shortcut-key text-outline">
                ⌘[
              </kbd>
            </button>
            <div className="w-px h-4 bg-outline-variant opacity-40" />
            <div className="flex items-center gap-space-sm min-w-0">
              <Icon name={session.name ? "auto_stories" : "schedule"} className="text-primary text-[20px]" />
              <SessionTitle session={session} onRenamed={onRenamed} variant="header" as="h1" />
            </div>
          </div>
          {/* Quick Session Status */}
          <div className="flex items-center gap-space-sm">
            <div className="flex items-center gap-1.5 px-space-sm py-1 rounded-full bg-surface-container font-label-sm text-label-sm text-on-surface-variant">
              {pending > 0 ? (
                <>
                  <Spinner className="h-3 w-3 text-secondary" />
                  <span>OCR: {pending} pending</span>
                </>
              ) : (
                <>
                  <span className="w-1.5 h-1.5 rounded-full bg-secondary" />
                  <span>{frames ? "Indexed & Queryable" : "Loading…"}</span>
                </>
              )}
            </div>
          </div>
        </div>
        {/* Metadata Line & Session Search Filter */}
        <div className="flex flex-wrap items-center justify-between gap-space-md pt-space-xs">
          <div className="flex flex-wrap items-center gap-space-sm font-label-md text-label-md text-outline">
            <div className="flex items-center gap-1 text-on-surface-variant">
              <Icon name="calendar_today" className="text-[15px]" />
              <span>{fmtWhen(session.started_at)}</span>
            </div>
            <span>•</span>
            <span className="px-2 py-0.5 rounded-md bg-surface-container text-on-surface-variant font-label-sm text-label-sm">
              {session.ended_at === null ? "Recording · " : "Duration: "}
              {fmtSpan(duration)}
            </span>
            <span className="px-2 py-0.5 rounded-md bg-surface-container text-on-surface-variant font-label-sm text-label-sm">
              {plural(frames?.length ?? session.frame_count, "frame")}
            </span>
          </div>
          {/* In-Session Realtime Filter Bar */}
          <div className="relative flex items-center w-full sm:w-80">
            <Icon name="filter_alt" className="absolute left-3 text-[16px] text-outline pointer-events-none" />
            <input
              ref={filterInput}
              className="w-full pl-9 pr-14 py-1.5 rounded-lg bg-surface-container text-on-surface placeholder:text-outline font-body-sm text-body-sm focus:outline-none focus:bg-surface-container-high shadow-inner select-text"
              placeholder="Search in this session (e.g. @Retryable)..."
              type="text"
              spellCheck={false}
              value={filter}
              onChange={(e) => {
                setFilter(e.target.value);
                setOpenId(null);
              }}
              onKeyDown={(e) => {
                if (e.key !== "Escape") return;
                e.stopPropagation(); // Esc here clears the filter instead of leaving the gallery
                if (filter) setFilter("");
                else e.currentTarget.blur();
              }}
              aria-label="Search in this session"
            />
            <kbd className="absolute right-2 px-1.5 py-0.5 rounded bg-surface-container-highest font-shortcut-key text-shortcut-key text-outline">
              ⌘F
            </kbd>
          </div>
        </div>
      </section>

      {/* Interactive Gallery Canvas */}
      <section className="flex flex-col px-space-xl py-space-lg gap-space-md">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-space-sm">
            <span className="font-headline-sm text-headline-sm font-medium text-on-surface">Timeline Snapshots</span>
            {visible && (
              <span className="px-2 py-0.5 rounded-full bg-secondary-container text-on-secondary-container font-label-sm text-label-sm font-semibold">
                {filterTerms.length ? `${count.toLocaleString()} ${count === 1 ? "match" : "matches"}` : plural(count, "frame")}
              </span>
            )}
          </div>
          <div className="flex items-center gap-space-xs font-label-sm text-label-sm text-outline">
            <span className="w-2 h-2 rounded-full bg-primary" />
            <span>Every 2s · near-duplicates skipped</span>
          </div>
        </div>

        {error && <p className="font-body-sm text-body-sm text-error">{error}</p>}
        {frames && frames.length === 0 && (
          <p className="font-body-md text-body-md text-outline">No frames were captured in this session.</p>
        )}
        {frames && frames.length > 0 && count === 0 && (
          <p className="font-body-md text-body-md text-outline">No frames in this session mention “{filter.trim()}”.</p>
        )}

        {/* Thumbnail Grid */}
        <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-space-md">
          {visible?.map((f) => {
            const inspected = f.id === lastId;
            const label = ocrLabel(f);
            const title = firstLine(f.ocr_text);
            return f.image_path ? (
              <div
                key={f.id}
                role="button"
                tabIndex={0}
                onClick={() => open(f.id)}
                onKeyDown={(e) => e.key === "Enter" && open(f.id)}
                className={`group relative flex flex-col rounded-xl bg-surface-container p-space-sm transition-all hover:bg-surface-container-high cursor-pointer ${
                  inspected ? "shadow-md ring-2 ring-primary" : "shadow-sm"
                }`}
              >
                <div className="relative w-full aspect-video rounded-lg overflow-hidden bg-surface-container-lowest flex items-center justify-center">
                  <img
                    className={`w-full h-full object-cover ${inspected ? "" : "opacity-90 group-hover:opacity-100 transition-opacity"}`}
                    src={convertFileSrc(f.image_path)}
                    loading="lazy"
                    decoding="async"
                    alt=""
                  />
                  {inspected && (
                    <>
                      <div className="absolute inset-0 bg-primary/10 mix-blend-overlay" />
                      <span className="absolute top-2 left-2 px-1.5 py-0.5 rounded bg-surface-container-lowest/80 backdrop-blur-md font-label-sm text-label-sm text-primary font-medium flex items-center gap-1 shadow-sm">
                        <Icon name="visibility" className="text-[12px] text-primary" />
                        Inspected
                      </span>
                    </>
                  )}
                  <span className="absolute bottom-2 right-2 px-1.5 py-0.5 rounded bg-surface-container-lowest/85 backdrop-blur-md font-label-sm text-label-sm text-on-surface font-semibold shadow-sm">
                    {fmtTime(f.ts)}
                  </span>
                </div>
                <div className="flex items-center justify-between gap-space-sm mt-space-sm">
                  <div className="flex items-center gap-1.5 min-w-0">
                    <Icon name="image" className={`text-[15px] ${inspected ? "text-primary" : "text-on-surface-variant"}`} />
                    <span className={`font-body-sm text-body-sm truncate ${title ? "text-on-surface" : "text-outline"}`}>
                      {title || (f.ocr_status === OCR_PENDING ? "Reading text…" : "No text")}
                    </span>
                  </div>
                  <span className={`font-label-sm text-label-sm shrink-0 ${inspected ? "text-primary-fixed-dim" : label.cls}`}>
                    {label.text}
                  </span>
                </div>
              </div>
            ) : (
              /* Pruned / Storage Retained State */
              <div
                key={f.id}
                role="button"
                tabIndex={0}
                onClick={() => open(f.id)}
                onKeyDown={(e) => e.key === "Enter" && open(f.id)}
                className={`group relative flex flex-col rounded-xl bg-surface-container-low p-space-sm shadow-sm hover:bg-surface-container transition-all cursor-pointer ${
                  inspected ? "ring-2 ring-primary" : ""
                }`}
              >
                <div className="relative w-full aspect-video rounded-lg bg-surface-container-lowest/70 flex flex-col items-center justify-center p-space-md text-center">
                  <div className="w-7 h-7 rounded-full bg-surface-container flex items-center justify-center mb-1 text-outline">
                    <Icon name="inventory_2" className="text-[16px]" />
                  </div>
                  <span className="font-body-sm text-body-sm font-medium text-on-surface-variant leading-tight">Image expired</span>
                  <span className="font-label-sm text-label-sm text-outline mt-0.5">text kept (OCR indexed)</span>
                  <span className="absolute bottom-2 right-2 px-1.5 py-0.5 rounded bg-surface-container/90 font-label-sm text-label-sm text-outline">
                    {fmtTime(f.ts)}
                  </span>
                </div>
                <div className="flex items-center justify-between gap-space-sm mt-space-sm">
                  <div className="flex items-center gap-1.5 min-w-0">
                    <Icon name="description" className="text-[15px] text-outline" />
                    <span className="font-body-sm text-body-sm text-outline truncate">{title || "No text"}</span>
                  </div>
                  <span className="font-label-sm text-label-sm text-outline shrink-0">Pruned</span>
                </div>
              </div>
            );
          })}
        </div>
      </section>

      {/* Frame Lightbox Overlay (Inspect Mode) */}
      {current && (
        <div
          className="fixed inset-0 z-[60] bg-black/60 backdrop-blur-sm flex items-center justify-center p-space-xl"
          onClick={() => setOpenId(null)}
        >
          <section
            className="flex flex-col w-full max-w-[1400px] h-full max-h-[900px] rounded-2xl bg-surface-container-lowest shadow-2xl overflow-hidden"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Lightbox Top bar */}
            <div className="flex items-center justify-between px-space-lg py-space-sm bg-surface-container-low shadow-sm">
              <div className="flex items-center gap-space-md">
                <div className="flex items-center gap-1.5">
                  <span className="w-2.5 h-2.5 rounded-full bg-primary animate-pulse" />
                  <span className="font-headline-sm text-headline-sm font-semibold text-on-surface">Inspect Frame</span>
                </div>
                <div className="flex items-center gap-space-xs font-label-md text-label-md">
                  <span className="px-2 py-0.5 rounded bg-surface-container-high text-primary font-semibold">
                    Frame {(index + 1).toLocaleString()} / {count.toLocaleString()}
                  </span>
                  <span className="text-outline">·</span>
                  <span className="text-on-surface-variant font-label-md text-label-md">{fmtTime(current.ts)}</span>
                </div>
              </div>
              <div className="flex items-center gap-space-sm">
                {/* Navigation Buttons */}
                <div className="flex items-center gap-1 bg-surface-container p-0.5 rounded-lg">
                  <button
                    className="p-1 rounded hover:bg-surface-container-high text-on-surface-variant hover:text-on-surface transition-colors flex items-center gap-1 text-body-sm disabled:opacity-40"
                    onClick={() => step(-1)}
                    disabled={index === 0}
                  >
                    <kbd className="px-1 py-0.5 rounded bg-surface-container-lowest font-shortcut-key text-shortcut-key text-outline">←</kbd>
                    <span className="font-label-sm text-label-sm">Prev</span>
                  </button>
                  <div className="w-px h-3 bg-outline-variant opacity-40" />
                  <button
                    className="p-1 rounded hover:bg-surface-container-high text-on-surface-variant hover:text-on-surface transition-colors flex items-center gap-1 text-body-sm disabled:opacity-40"
                    onClick={() => step(1)}
                    disabled={index === count - 1}
                  >
                    <span className="font-label-sm text-label-sm">Next</span>
                    <kbd className="px-1 py-0.5 rounded bg-surface-container-lowest font-shortcut-key text-shortcut-key text-outline">→</kbd>
                  </button>
                </div>
                <button
                  className="p-1.5 rounded-lg bg-surface-container hover:bg-surface-container-high text-outline hover:text-on-surface transition-colors flex items-center"
                  title="Close Lightbox (Esc)"
                  aria-label="Close"
                  onClick={() => setOpenId(null)}
                >
                  <Icon name="close" className="text-[18px]" />
                </button>
              </div>
            </div>

            {/* Lightbox Main Body (Split View) */}
            <div className="flex flex-col lg:flex-row flex-1 min-h-0">
              {/* Left Screen Preview Canvas */}
              <div className="relative flex-1 min-w-0 min-h-0 bg-surface flex flex-col gap-space-sm p-space-lg group">
                {/* Overlay Badges */}
                <div className="flex items-center justify-between w-full z-10">
                  <div className="flex items-center gap-space-xs px-space-sm py-1 rounded-full bg-surface-container-lowest/80 backdrop-blur-md text-on-surface-variant shadow-sm">
                    <Icon name="desktop_windows" className="text-[15px] text-primary" />
                    <span className="font-label-sm text-label-sm">
                      {current.image_path ? (dims ? `Screenshot · ${dims.w} × ${dims.h}` : "Screenshot") : "Image expired"}
                    </span>
                  </div>
                  {current.image_path && (
                    <div className="flex items-center gap-space-xs">
                      <button
                        className={`px-2 py-1 rounded-md backdrop-blur-md transition-colors font-label-sm text-label-sm flex items-center gap-1 shadow-sm ${
                          fit ? "bg-surface-container-lowest/80 text-on-surface hover:bg-surface-container-high" : "bg-surface-container-high text-primary"
                        }`}
                        onClick={() => setFit(false)}
                      >
                        <Icon name="zoom_in" className="text-[14px]" />
                        100%
                      </button>
                      <button
                        className={`px-2 py-1 rounded-md backdrop-blur-md transition-colors font-label-sm text-label-sm flex items-center gap-1 shadow-sm ${
                          fit ? "bg-surface-container-high text-primary" : "bg-surface-container-lowest/80 text-on-surface hover:bg-surface-container-high"
                        }`}
                        onClick={() => setFit(true)}
                      >
                        <Icon name="crop_free" className="text-[14px]" />
                        Fit
                      </button>
                    </div>
                  )}
                </div>
                {/* Rendered Screen Snapshot */}
                <div
                  className={`relative flex-1 min-h-0 rounded-xl bg-surface-container-low shadow-xl ${
                    fit ? "flex items-center justify-center overflow-hidden" : "overflow-auto"
                  }`}
                >
                  {current.image_path ? (
                    <img
                      key={current.id}
                      src={convertFileSrc(current.image_path)}
                      alt=""
                      className={fit ? "max-w-full max-h-full object-contain" : "max-w-none"}
                      onLoad={(e) => setDims({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })}
                    />
                  ) : (
                    <div className="h-full flex flex-col items-center justify-center text-center gap-1">
                      <div className="w-10 h-10 rounded-full bg-surface-container flex items-center justify-center mb-1 text-outline">
                        <Icon name="inventory_2" className="text-[22px]" />
                      </div>
                      <span className="font-body-md text-body-md font-medium text-on-surface-variant">Image expired</span>
                      <span className="font-label-sm text-label-sm text-outline">The text was kept and is still searchable</span>
                    </div>
                  )}
                </div>
                {/* Footer of the canvas */}
                <div className="flex items-center justify-between z-10">
                  <div className="flex items-center gap-space-xs font-label-sm text-label-sm text-outline">
                    <Icon name="pin_drop" className="text-[14px] text-primary" />
                    <span>
                      {hlTerms.length
                        ? `Highlighting ${plural(occurrences, "occurrence")} of “${hlTerms.join(" ")}” in the text`
                        : fmtWhen(current.ts)}
                    </span>
                  </div>
                  <span className="font-label-sm text-label-sm text-outline">Captured via ScreenCaptureKit</span>
                </div>
              </div>
              {/* Right OCR Inspection & Data Panel (340px) */}
              <OcrPanel frame={current} terms={hlTerms} />
            </div>

            {/* Keyboard Navigation Footer Hints */}
            <div className="flex items-center justify-between px-space-lg py-2.5 bg-surface-container-low font-label-sm text-label-sm text-outline">
              <div className="flex items-center gap-space-md">
                <span className="flex items-center gap-1">
                  <kbd className="px-1.5 py-0.5 rounded bg-surface-container font-shortcut-key text-shortcut-key text-on-surface-variant">Esc</kbd>
                  <span>close</span>
                </span>
                <span className="flex items-center gap-1">
                  <kbd className="px-1.5 py-0.5 rounded bg-surface-container font-shortcut-key text-shortcut-key text-on-surface-variant">←</kbd>
                  <kbd className="px-1.5 py-0.5 rounded bg-surface-container font-shortcut-key text-shortcut-key text-on-surface-variant">→</kbd>
                  <span>navigate timeline</span>
                </span>
                <span className="flex items-center gap-1">
                  <kbd className="px-1.5 py-0.5 rounded bg-surface-container font-shortcut-key text-shortcut-key text-on-surface-variant">⌘C</kbd>
                  <span>copy text</span>
                </span>
              </div>
              <div className="flex items-center gap-space-xs text-primary font-medium">
                <Icon name="shield" className="text-[14px]" />
                <span>Zero cloud upload · Stored only on this Mac</span>
              </div>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
