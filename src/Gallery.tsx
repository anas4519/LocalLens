import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { FrameRow, Session, listFrames } from "./api";

const fmtTime = (ts: number) =>
  new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

function Placeholder({ large = false }: { large?: boolean }) {
  return (
    <div className={`placeholder ${large ? "large" : ""}`}>
      <svg viewBox="0 0 24 24" width={large ? 36 : 22} height={large ? 36 : 22} aria-hidden>
        <path
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          d="M4 5h16v14H4zM4 15l4-4 4 4 3-3 5 5M3 3l18 18"
        />
      </svg>
      <span>Image expired · text kept</span>
    </div>
  );
}

export default function Gallery({ session, onBack }: { session: Session; onBack: () => void }) {
  const [frames, setFrames] = useState<FrameRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<number | null>(null);

  useEffect(() => {
    listFrames(session.id).then(setFrames).catch((e) => setError(String(e)));
  }, [session.id]);

  const count = frames?.length ?? 0;
  const step = useCallback(
    (d: number) => setOpen((i) => (i === null ? i : Math.min(count - 1, Math.max(0, i + d)))),
    [count],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (open === null) {
        if (e.key === "Escape") onBack();
        return;
      }
      if (e.key === "Escape") setOpen(null);
      else if (e.key === "ArrowRight") step(1);
      else if (e.key === "ArrowLeft") step(-1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, step, onBack]);

  const current = open !== null && frames ? frames[open] : null;

  return (
    <main className="app gallery-view">
      <header className="gallery-header">
        <button className="ghost" onClick={onBack}>
          ← Sessions
        </button>
        <div>
          <h1>{new Date(session.started_at).toLocaleString()}</h1>
          <p className="tagline">
            {frames ? `${count} ${count === 1 ? "frame" : "frames"}` : "Loading…"}
          </p>
        </div>
      </header>

      {error && <p className="error">{error}</p>}
      {frames && count === 0 && <p className="empty">No frames were captured in this session.</p>}

      <div className="grid">
        {frames?.map((f, i) => (
          <button key={f.id} className="thumb" onClick={() => setOpen(i)}>
            {f.image_path ? (
              <img src={convertFileSrc(f.image_path)} loading="lazy" decoding="async" alt="" />
            ) : (
              <Placeholder />
            )}
            <span className="ts">{fmtTime(f.ts)}</span>
          </button>
        ))}
      </div>

      {current && open !== null && (
        <div className="lightbox" onClick={() => setOpen(null)}>
          <div className="lightbox-inner" onClick={(e) => e.stopPropagation()}>
            {current.image_path ? (
              <img src={convertFileSrc(current.image_path)} alt="" />
            ) : (
              <Placeholder large />
            )}
            <div className="lightbox-bar">
              <button className="ghost" onClick={() => step(-1)} disabled={open === 0}>
                ←
              </button>
              <span>
                {fmtTime(current.ts)} · {open + 1} / {count}
              </span>
              <button className="ghost" onClick={() => step(1)} disabled={open === count - 1}>
                →
              </button>
            </div>
          </div>
          <button className="ghost close" onClick={() => setOpen(null)} aria-label="Close">
            ✕
          </button>
        </div>
      )}
    </main>
  );
}
