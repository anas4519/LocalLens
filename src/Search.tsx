import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Placeholder, fmtTime } from "./Gallery";
import { fmtWhen } from "./SessionTitle";
import { SearchHit, search } from "./api";

const DEBOUNCE_MS = 200;

interface Props {
  /** Lives in App so it survives a trip into the gallery and back. */
  query: string;
  onQueryChange: (q: string) => void;
  onOpen: (hit: SearchHit) => void;
}

/** True when a keypress is going into a text field, so "/" should type, not jump. */
const isTyping = (t: EventTarget | null) =>
  t instanceof HTMLElement && (t.isContentEditable || t.tagName === "INPUT" || t.tagName === "TEXTAREA");

function Snippet({ hit }: { hit: SearchHit }) {
  // OCR text is untrusted: every part is a React text node, so "<script>" just shows as text.
  return (
    <p className="snippet">
      {hit.snippet.map((p, i) => (p.hit ? <mark key={i}>{p.text}</mark> : <span key={i}>{p.text}</span>))}
    </p>
  );
}

export default function Search({ query, onQueryChange, onOpen }: Props) {
  const [hits, setHits] = useState<SearchHit[] | null>(null); // null = nothing searched yet
  const [error, setError] = useState<string | null>(null);
  const [sel, setSel] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const latest = useRef(0); // id of the newest request; older answers are dropped

  // Debounced live search. Each keystroke cancels the pending timer, and a slow
  // response for an old query can't overwrite the results of a newer one.
  useEffect(() => {
    const q = query.trim();
    const id = ++latest.current;
    if (!q) {
      setHits(null);
      setError(null);
      return;
    }
    const t = setTimeout(() => {
      search(q)
        .then((r) => {
          if (id !== latest.current) return;
          setHits(r);
          setSel(0);
          setError(null);
        })
        .catch((e) => id === latest.current && setError(String(e)));
    }, DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [query]);

  // Cmd+K (Ctrl+K) from anywhere, or "/" when not already typing, focuses the bar.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        input.current?.focus();
        input.current?.select();
      } else if (e.key === "/" && !isTyping(e.target)) {
        e.preventDefault();
        input.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Keep the keyboard-selected result visible.
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${sel}"]`)?.scrollIntoView({ block: "nearest" });
  }, [sel]);

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
    } else if (e.key === "Enter" && hits?.[sel]) {
      onOpen(hits[sel]);
    }
  };

  const q = query.trim();

  return (
    <section className="search">
      <div className="search-bar">
        <svg className="lens" viewBox="0 0 24 24" width="16" height="16" aria-hidden>
          <path
            fill="none"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
            d="M10.5 18a7.5 7.5 0 1 1 0-15 7.5 7.5 0 0 1 0 15zM16 16l5 5"
          />
        </svg>
        <input
          ref={input}
          className="search-input"
          type="search"
          value={query}
          placeholder="Search everything you've captured"
          spellCheck={false}
          autoComplete="off"
          onChange={(e) => onQueryChange(e.target.value)}
          onKeyDown={onKeyDown}
          aria-label="Search captured text"
          aria-controls="search-results"
        />
        {!query && <kbd className="kbd">⌘K</kbd>}
      </div>

      {q && error && <p className="error">{error}</p>}
      {q && hits && hits.length === 0 && !error && (
        <p className="search-empty">No matches for “{q}”. Text appears here once OCR has read a frame.</p>
      )}
      {q && hits && hits.length > 0 && (
        <ul className="results" id="search-results" role="listbox" ref={list}>
          {hits.map((h, i) => (
            <li
              key={h.frame_id}
              data-index={i}
              role="option"
              aria-selected={i === sel}
              className={`result ${i === sel ? "selected" : ""}`}
              onMouseEnter={() => setSel(i)}
              onClick={() => onOpen(h)}
            >
              <div className="result-thumb">
                {h.image_path ? (
                  <img src={convertFileSrc(h.image_path)} loading="lazy" decoding="async" alt="" />
                ) : (
                  <Placeholder />
                )}
              </div>
              <div className="result-body">
                <Snippet hit={h} />
                <span className="result-meta">
                  <strong>{h.session_name ?? fmtWhen(h.session_started_at)}</strong>
                  {" · "}
                  {h.session_name ? fmtWhen(h.ts) : fmtTime(h.ts)}
                </span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
