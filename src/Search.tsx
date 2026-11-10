import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { SearchHit, search } from "./api";
import { dayLabel, fmtSpan, fmtTime, fmtWhen } from "./format";
import { Marked } from "./highlight";
import { Icon, Spinner, hasTextSelection } from "./ui";

const DEBOUNCE_MS = 200;
const LIMIT = 50; // backend caps at 100; we say "50+" when we hit it

interface Props {
  /** Lives in App so it survives a trip into the gallery and back. */
  query: string;
  onQueryChange: (q: string) => void;
  /** Open the hit's session with the lightbox on that frame (↵ / click). */
  onOpen: (hit: SearchHit) => void;
  /** Open the hit's session gallery without jumping (⌘↵). */
  onOpenSession: (hit: SearchHit) => void;
  /** Bumped by App on ⌘K or "/" so the input focuses even if this screen was already showing. */
  focusSignal: number;
}

const sessionTitle = (h: SearchHit) => h.session_name ?? fmtWhen(h.session_started_at);
const snippetText = (h: SearchHit) => h.snippet.map((p) => p.text).join("");

/** OCR text is untrusted: every part is a React text node, so "<script>" just shows as text. */
function Snippet({ hit, markClass }: { hit: SearchHit; markClass: string }) {
  return <Marked parts={hit.snippet} markClass={markClass} />;
}

const kbd = "px-1.5 py-0.5 rounded bg-surface-container font-shortcut-key text-shortcut-key text-outline";

export default function Search({ query, onQueryChange, onOpen, onOpenSession, focusSignal }: Props) {
  const [hits, setHits] = useState<SearchHit[] | null>(null); // null = nothing searched yet
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [tookMs, setTookMs] = useState<number | null>(null);
  const [sel, setSel] = useState(0);
  const [copied, setCopied] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const latest = useRef(0); // id of the newest request; older answers are dropped

  // Debounced live search. Each keystroke cancels the pending timer, and a slow
  // response for an old query can't overwrite the results of a newer one.
  useEffect(() => {
    const q = query.trim();
    const id = ++latest.current;
    if (!q) {
      setHits(null);
      setError(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    const t = setTimeout(() => {
      const t0 = performance.now();
      search(q, LIMIT)
        .then((r) => {
          if (id !== latest.current) return;
          setHits(r);
          setSel(0);
          setError(null);
          setTookMs(performance.now() - t0);
        })
        .catch((e) => id === latest.current && setError(String(e)))
        .finally(() => id === latest.current && setLoading(false));
    }, DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [query]);

  // Focus on arrival and whenever App asks (⌘K / "/" handled globally in App).
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [focusSignal]);

  // Keep the keyboard-selected result visible.
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${sel}"]`)?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  const current = hits?.[sel] ?? null;

  const copy = useCallback(async () => {
    if (!current) return;
    try {
      await navigator.clipboard.writeText(snippetText(current));
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* clipboard may be denied; the excerpt is selectable */
    }
  }, [current]);

  // ⌘C copies the selected excerpt unless some text is selected (then it's a normal copy).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "c" && current && !hasTextSelection()) {
        e.preventDefault();
        copy();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [copy, current]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    const n = hits?.length ?? 0;
    if (e.key === "Escape") {
      if (query) onQueryChange("");
      else input.current?.blur();
    } else if (e.key === "ArrowDown" && n > 0) {
      e.preventDefault();
      setSel((i) => Math.min(n - 1, i + 1));
    } else if (e.key === "ArrowUp" && n > 0) {
      e.preventDefault();
      setSel((i) => Math.max(0, i - 1));
    } else if (e.key === "Enter" && current) {
      if (e.metaKey || e.ctrlKey) onOpenSession(current);
      else onOpen(current);
    }
  };

  const q = query.trim();
  const n = hits?.length ?? 0;
  const sessionsHit = new Set(hits?.map((h) => h.session_id)).size;
  const countLabel = n >= LIMIT ? `${LIMIT}+` : `${n}`;

  return (
    <div className="flex flex-col w-full h-[calc(100vh-3.5rem)] overflow-hidden bg-surface">
      {/* Query bar */}
      <div className="px-space-xl pt-space-lg pb-space-md flex flex-col gap-space-sm bg-surface-container-low shadow-sm">
        <div className="flex items-center justify-between gap-space-md">
          <div className="relative flex-1 flex items-center bg-surface-container-highest/70 rounded-xl px-space-md py-2.5 shadow-[0_0_0_1px_rgba(202,190,255,0.4),0_0_18px_rgba(148,125,255,0.18)]">
            <Icon name="manage_search" className="text-primary text-[20px] mr-space-sm" />
            <input
              ref={input}
              className="bg-transparent border-0 outline-none w-full text-on-surface font-headline-sm text-headline-sm placeholder:text-outline tracking-tight select-text"
              type="text"
              value={query}
              placeholder="Type to search everything you've captured..."
              spellCheck={false}
              autoComplete="off"
              onChange={(e) => onQueryChange(e.target.value)}
              onKeyDown={onKeyDown}
              aria-label="Search captured text"
              aria-controls="search-results"
            />
            <div className="flex items-center gap-space-xs ml-space-sm shrink-0">
              {loading && <Spinner className="h-3 w-3 text-secondary" />}
              {q && hits && (
                <>
                  <span className="font-label-sm text-label-sm text-primary bg-surface-container-high px-1.5 py-0.5 rounded font-medium">
                    {countLabel} {n === 1 ? "match" : "matches"}
                  </span>
                  {n > 0 && (
                    <span className="font-label-sm text-label-sm text-outline">
                      in {sessionsHit} {sessionsHit === 1 ? "session" : "sessions"}
                    </span>
                  )}
                </>
              )}
              <kbd className="ml-1 px-1.5 py-0.5 rounded bg-surface-container font-shortcut-key text-shortcut-key text-outline">⌘K</kbd>
            </div>
          </div>
        </div>
        <div className="flex items-center justify-between px-space-xs">
          <div className="flex items-center gap-space-xs text-outline font-label-sm text-label-sm">
            <span className="w-1.5 h-1.5 rounded-full bg-primary animate-pulse" />
            <span>ON-DEVICE FULL-TEXT INDEX</span>
            <span className="text-surface-variant">/</span>
            <span>OCR TEXT MATCH · SQLITE FTS5</span>
          </div>
          <span className="font-label-sm text-label-sm text-outline">Sort: Relevance (BM25), newest first on ties</span>
        </div>
      </div>

      <div className="flex-1 flex min-h-0 overflow-hidden">
        {/* Results */}
        <div
          className="w-[52%] flex flex-col overflow-y-auto p-space-md gap-space-xs bg-surface"
          id="search-results"
          role="listbox"
          ref={list}
        >
          {!q && (
            <div className="flex flex-col gap-space-md p-space-md text-on-surface-variant">
              <span className="font-headline-sm text-headline-sm font-semibold text-on-surface">
                Search everything you've read
              </span>
              <ul className="flex flex-col gap-space-sm font-body-md text-body-md">
                <li className="flex items-start gap-space-sm">
                  <Icon name="bolt" className="text-[16px] text-primary" />
                  Results update as you type. Half a word works: “deplo” finds “deploy”.
                </li>
                <li className="flex items-start gap-space-sm">
                  <Icon name="join" className="text-[16px] text-primary" />
                  Every word must appear in the frame. Quotes, “-”, AND and OR are just text.
                </li>
                <li className="flex items-start gap-space-sm">
                  <Icon name="keyboard" className="text-[16px] text-primary" />
                  ↑↓ to pick, ↵ to open the frame, ⌘↵ to open its session. ⌘K or / from anywhere.
                </li>
              </ul>
            </div>
          )}
          {q && error && <p className="p-space-md font-body-sm text-body-sm text-error">{error}</p>}
          {q && hits && n === 0 && !error && !loading && (
            <div className="flex flex-col items-center text-center gap-space-sm p-space-xl text-outline">
              <Icon name="search_off" className="text-[28px]" />
              <span className="font-body-md text-body-md text-on-surface-variant">No matches for “{q}”.</span>
              <span className="font-body-sm text-body-sm">Text appears here once OCR has read a frame.</span>
            </div>
          )}
          {q &&
            hits?.map((h, i) => {
              const selected = i === sel;
              return (
                <div
                  key={h.frame_id}
                  data-index={i}
                  role="option"
                  aria-selected={selected}
                  className={`search-row group relative cursor-pointer rounded-xl p-space-md transition-all ${
                    selected
                      ? "bg-surface-container-high/90 text-on-surface shadow-md"
                      : "bg-surface-container-low/50 hover:bg-surface-container-high/60 text-on-surface-variant hover:text-on-surface"
                  }`}
                  onMouseEnter={() => setSel(i)}
                  onClick={() => onOpen(h)}
                >
                  {selected && <div className="absolute left-0 top-3 bottom-3 w-1 bg-primary rounded-r-full" />}
                  <div className="flex items-center justify-between gap-space-sm mb-space-xs pl-space-xs">
                    <div className="flex items-center gap-space-sm min-w-0">
                      <span
                        className={`p-1 rounded-md bg-surface-container-lowest flex items-center justify-center ${
                          selected ? "text-primary" : "text-outline group-hover:text-primary"
                        }`}
                      >
                        <Icon name={selected ? "travel_explore" : h.image_path ? "image" : "description"} className="text-[16px]" />
                      </span>
                      <span className="font-body-md text-body-md font-medium text-on-surface truncate">{sessionTitle(h)}</span>
                    </div>
                    <div className="flex items-center gap-space-xs shrink-0">
                      <span
                        className={`font-label-sm text-label-sm px-1.5 py-0.5 rounded ${
                          selected ? "text-primary bg-primary/10" : "text-on-surface-variant bg-surface-container-highest"
                        }`}
                      >
                        {dayLabel(h.ts)}
                      </span>
                      <span className={`font-label-sm text-label-sm text-outline ${selected ? "font-medium" : ""}`}>
                        {fmtTime(h.ts)}
                        {selected ? ` · ${fmtSpan(h.ts - h.session_started_at)} in` : ""}
                      </span>
                    </div>
                  </div>
                  <p className="pl-space-xs font-body-sm text-body-sm text-on-surface-variant line-clamp-2 leading-relaxed">
                    <Snippet
                      hit={h}
                      markClass={`text-secondary font-medium px-1 py-0.5 rounded ${
                        selected ? "bg-secondary-container" : "bg-secondary-container/60"
                      }`}
                    />
                  </p>
                  <div className="pl-space-xs mt-space-sm flex items-center justify-between text-outline">
                    <div className="flex items-center gap-space-md">
                      <span className="flex items-center gap-1 font-label-sm text-label-sm">
                        <Icon name="memory" className="text-[14px]" />
                        Frame #{h.frame_id.toLocaleString()}
                      </span>
                      {!h.image_path && (
                        <span className="flex items-center gap-1 font-label-sm text-label-sm">
                          <Icon name="inventory_2" className="text-[14px]" />
                          Image expired · text kept
                        </span>
                      )}
                    </div>
                    {selected ? (
                      <span className="font-shortcut-key text-shortcut-key text-primary flex items-center gap-1 opacity-90">
                        <span>selected</span>
                        <kbd className="px-1 py-0.2 rounded bg-surface-container font-shortcut-key text-[10px]">↵</kbd>
                      </span>
                    ) : (
                      <span className="font-label-sm text-label-sm text-outline opacity-0 group-hover:opacity-100 transition-opacity">
                        Open frame
                      </span>
                    )}
                  </div>
                </div>
              );
            })}
        </div>

        {/* Frame inspector */}
        <div className="w-[48%] flex flex-col bg-surface-container-low overflow-y-auto p-space-lg gap-space-md">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-space-xs">
              <span className="font-label-sm text-label-sm text-outline uppercase tracking-wider">FRAME INSPECTOR</span>
              <span className="font-label-sm text-label-sm text-primary bg-primary/10 px-1.5 py-0.5 rounded">ON-DEVICE OCR</span>
            </div>
            {current && (
              <button
                className="flex items-center gap-1 px-space-md py-1.5 rounded-lg bg-primary text-on-primary font-headline-sm text-[13px] hover:bg-primary-container hover:text-on-primary-container transition-colors shadow-sm"
                onClick={() => onOpen(current)}
              >
                <Icon name="fullscreen" className="text-[16px]" />
                <span>↵ Open in Lightbox</span>
              </button>
            )}
          </div>

          {!current ? (
            <div className="flex-1 flex flex-col items-center justify-center text-center gap-space-sm text-outline">
              <Icon name="frame_inspect" className="text-[32px]" />
              <span className="font-body-md text-body-md">
                {q ? "Pick a result to preview it here." : "Search results show a preview here."}
              </span>
            </div>
          ) : (
            <>
              <div className="relative w-full aspect-video rounded-xl overflow-hidden bg-surface-container-lowest shadow-xl group">
                {current.image_path ? (
                  <img className="w-full h-full object-cover object-top" src={convertFileSrc(current.image_path)} alt="" />
                ) : (
                  <div className="w-full h-full flex flex-col items-center justify-center text-center gap-1 text-outline">
                    <Icon name="inventory_2" className="text-[22px]" />
                    <span className="font-body-sm text-body-sm text-on-surface-variant">Image expired · text kept</span>
                  </div>
                )}
                <div className="absolute inset-0 bg-gradient-to-t from-surface-dim/80 via-transparent to-transparent pointer-events-none" />
                <div className="absolute bottom-3 left-3 right-3 flex items-center justify-between text-on-surface">
                  <div className="flex items-center gap-space-xs bg-surface-dim/90 backdrop-blur px-2.5 py-1 rounded-lg">
                    <Icon name="schedule" className="text-primary text-[15px]" />
                    <span className="font-label-sm text-label-sm font-medium">
                      {fmtTime(current.ts)} · Frame #{current.frame_id}
                    </span>
                  </div>
                </div>
              </div>

              <div className="flex flex-col gap-space-xs bg-surface-container rounded-xl p-space-md shadow-sm">
                <div className="flex items-center justify-between pb-space-xs">
                  <span className="font-label-sm text-label-sm uppercase tracking-wider text-outline flex items-center gap-1">
                    <Icon name="text_snippet" className="text-[14px] text-primary" />
                    Matched Excerpt
                  </span>
                  <button
                    className="flex items-center gap-1 font-body-sm text-body-sm text-primary hover:text-primary-container transition-colors"
                    onClick={copy}
                  >
                    <Icon name={copied ? "check" : "content_copy"} className="text-[14px]" />
                    <span>{copied ? "Copied" : "⌘C Copy Text"}</span>
                  </button>
                </div>
                <div className="p-space-sm rounded-lg bg-surface-container-lowest font-label-md text-label-md text-on-surface leading-relaxed select-text overflow-x-auto">
                  <p className="whitespace-pre-wrap break-words">
                    <Snippet hit={current} markClass="bg-secondary-container text-secondary font-semibold px-1 rounded" />
                  </p>
                </div>
              </div>

              <div className="grid grid-cols-2 gap-space-sm">
                <div className="flex flex-col gap-1 p-space-sm rounded-xl bg-surface-container min-w-0">
                  <span className="font-label-sm text-label-sm text-outline">SESSION</span>
                  <span className="font-body-md text-body-md text-on-surface font-medium truncate">{sessionTitle(current)}</span>
                  <span className="font-label-sm text-label-sm text-outline truncate">
                    Started {fmtWhen(current.session_started_at)}
                  </span>
                </div>
                <div className="flex flex-col gap-1 p-space-sm rounded-xl bg-surface-container min-w-0">
                  <span className="font-label-sm text-label-sm text-outline">CAPTURED</span>
                  <span className="font-body-md text-body-md text-on-surface font-medium truncate">{fmtWhen(current.ts)}</span>
                  <span className="font-label-sm text-label-sm text-primary truncate">
                    {fmtSpan(current.ts - current.session_started_at)} into the session · On-device indexed
                  </span>
                </div>
              </div>

              <div className="flex items-center justify-end pt-space-xs">
                <button
                  className="flex items-center gap-space-xs text-primary hover:text-primary-container text-body-sm font-body-sm transition-colors"
                  onClick={() => onOpenSession(current)}
                >
                  <Icon name="open_in_new" className="text-[16px]" />
                  <span>⌘↵ Open Session</span>
                </button>
              </div>
            </>
          )}
        </div>
      </div>

      {/* Footer */}
      <footer className="h-10 px-space-xl bg-surface-container-lowest flex items-center justify-between text-outline shadow-[0_-1px_0_0_rgba(255,255,255,0.04)] select-none shrink-0">
        <div className="flex items-center gap-space-lg font-body-sm text-body-sm">
          <div className="flex items-center gap-1.5">
            <kbd className={kbd}>↑</kbd>
            <kbd className={kbd}>↓</kbd>
            <span>Navigate</span>
          </div>
          <span className="text-surface-variant">·</span>
          <div className="flex items-center gap-1.5">
            <kbd className={kbd}>↵</kbd>
            <span>Open Lightbox</span>
          </div>
          <span className="text-surface-variant">·</span>
          <div className="flex items-center gap-1.5">
            <kbd className={kbd}>⌘C</kbd>
            <span>Copy Text</span>
          </div>
          <span className="text-surface-variant">·</span>
          <div className="flex items-center gap-1.5">
            <kbd className={kbd}>⌘↵</kbd>
            <span>Open Session</span>
          </div>
          <span className="text-surface-variant">·</span>
          <div className="flex items-center gap-1.5">
            <kbd className={kbd}>Esc</kbd>
            <span>Clear</span>
          </div>
        </div>
        <div className="flex items-center gap-space-sm">
          {q && hits && tookMs !== null && (
            <span className="font-label-sm text-label-sm text-secondary bg-secondary-container/20 px-2 py-0.5 rounded-full flex items-center gap-1 font-medium">
              <span>⚡</span> {countLabel} results searched locally in {tookMs.toFixed(1)}ms
            </span>
          )}
          <span className="w-1.5 h-1.5 rounded-full bg-[#10b981]" />
          <span className="font-label-sm text-label-sm text-[#10b981]">Zero network</span>
        </div>
      </footer>
    </div>
  );
}
