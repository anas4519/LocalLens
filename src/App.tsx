import { useCallback, useEffect, useState } from "react";
import Gallery from "./Gallery";
import Search from "./Search";
import SessionTitle from "./SessionTitle";
import {
  MAX_SESSION_NAME,
  SearchHit,
  Session,
  captureStatus,
  listSessions,
  startSession,
  stopSession,
} from "./api";

function fmtDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => n.toString().padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(sec)}` : `${pad(m)}:${pad(sec)}`;
}

export default function App() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [active, setActive] = useState<Session | null>(null);
  const [now, setNow] = useState(Date.now());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [hasPermission, setHasPermission] = useState(true);
  const [viewing, setViewing] = useState<Session | null>(null);
  const [captureError, setCaptureError] = useState<string | null>(null);
  const [ocrPending, setOcrPending] = useState(0);
  const [newName, setNewName] = useState("");
  const [query, setQuery] = useState("");
  const [jumpFrame, setJumpFrame] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      const all = await listSessions();
      setSessions(all);
      setActive(all.find((s) => s.ended_at === null) ?? null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const pollStatus = useCallback(async () => {
    try {
      const st = await captureStatus();
      setHasPermission(st.has_permission);
      setCaptureError(st.capture_error);
      setActive(st.active);
      setOcrPending(st.ocr_pending);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    pollStatus();
  }, [refresh, pollStatus]);

  // Live frame count: poll while recording; re-check permission every few seconds when idle.
  useEffect(() => {
    const t = setInterval(pollStatus, active || ocrPending > 0 ? 2000 : 5000);
    return () => clearInterval(t);
  }, [active, ocrPending, pollStatus]);

  useEffect(() => {
    if (!active) return;
    const t = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(t);
  }, [active]);

  const toggle = async () => {
    setBusy(true);
    setError(null);
    try {
      if (active) await stopSession();
      else {
        await startSession(newName);
        setNewName("");
      }
      setNow(Date.now());
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const past = sessions.filter((s) => s.ended_at !== null);

  // One place to apply a rename result, wherever it came from (list, gallery, active card).
  const applyRename = (s: Session) => {
    setSessions((all) => all.map((x) => (x.id === s.id ? s : x)));
    setActive((a) => (a && a.id === s.id ? s : a));
    setViewing((v) => (v && v.id === s.id ? s : v));
  };

  const openSession = (s: Session) => {
    setJumpFrame(null);
    setViewing(s);
  };

  // A search hit only carries the session id; get the full session (refetch if it's new).
  const openHit = async (h: SearchHit) => {
    try {
      let s = sessions.find((x) => x.id === h.session_id);
      if (!s) {
        const all = await listSessions();
        setSessions(all);
        s = all.find((x) => x.id === h.session_id);
      }
      if (!s) return setError("That session no longer exists.");
      setJumpFrame(h.frame_id);
      setViewing(s);
    } catch (e) {
      setError(String(e));
    }
  };

  if (viewing)
    return (
      <Gallery
        session={viewing}
        initialFrameId={jumpFrame}
        onBack={() => setViewing(null)}
        onRenamed={applyRename}
      />
    );

  return (
    <main className="app">
      <header className="header">
        <h1>LocalLens</h1>
        <p className="tagline">Private memory for the sessions you choose. Nothing leaves this Mac.</p>
      </header>

      <Search query={query} onQueryChange={setQuery} onOpen={openHit} />

      {!hasPermission && (
        <section className="notice">
          <strong>Screen Recording permission needed</strong>
          <p>
            Open System Settings → Privacy &amp; Security → Screen Recording, enable LocalLens (or your terminal in dev
            mode), then quit and reopen the app.
          </p>
        </section>
      )}

      <section className="capture">
        <div className={`timer ${active ? "live" : ""}`}>
          {active ? fmtDuration(now - active.started_at) : "00:00"}
        </div>
        {active ? (
          <SessionTitle session={active} onRenamed={applyRename} />
        ) : (
          <input
            className="name-input"
            value={newName}
            maxLength={MAX_SESSION_NAME}
            placeholder="Session name (optional)"
            disabled={busy}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && !busy && toggle()}
            aria-label="Session name (optional)"
          />
        )}
        <button className={`primary ${active ? "stop" : "start"}`} onClick={toggle} disabled={busy}>
          {active ? "Stop session" : "Start session"}
        </button>
        <p className="status">
          {active ? `Recording · ${active.frame_count} ${active.frame_count === 1 ? "frame" : "frames"}` : "Idle"}
          {ocrPending > 0 ? ` · OCR: ${ocrPending} pending` : ""}
        </p>
        {captureError && <p className="error">Capture problem: {captureError}</p>}
        {error && <p className="error">{error}</p>}
      </section>

      <section className="history">
        <h2>Past sessions</h2>
        {past.length === 0 ? (
          <p className="empty">No sessions yet. Start one above.</p>
        ) : (
          <ul>
            {past.map((s) => (
              <li key={s.id} className="clickable" onClick={() => openSession(s)}>
                <SessionTitle session={s} onRenamed={applyRename} />
                <span className="meta">
                  {fmtDuration((s.ended_at ?? s.started_at) - s.started_at)} · {s.frame_count} frames
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  );
}
