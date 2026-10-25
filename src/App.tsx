import { useCallback, useEffect, useState } from "react";
import { Session, listSessions, startSession, stopSession } from "./api";

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

  const refresh = useCallback(async () => {
    try {
      const all = await listSessions();
      setSessions(all);
      setActive(all.find((s) => s.ended_at === null) ?? null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

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
      else await startSession();
      setNow(Date.now());
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const past = sessions.filter((s) => s.ended_at !== null);

  return (
    <main className="app">
      <header className="header">
        <h1>LocalLens</h1>
        <p className="tagline">Private memory for the sessions you choose. Nothing leaves this Mac.</p>
      </header>

      <section className="capture">
        <div className={`timer ${active ? "live" : ""}`}>
          {active ? fmtDuration(now - active.started_at) : "00:00"}
        </div>
        <button className={`primary ${active ? "stop" : "start"}`} onClick={toggle} disabled={busy}>
          {active ? "Stop session" : "Start session"}
        </button>
        <p className="status">{active ? "Recording session…" : "Idle"}</p>
        {error && <p className="error">{error}</p>}
      </section>

      <section className="history">
        <h2>Past sessions</h2>
        {past.length === 0 ? (
          <p className="empty">No sessions yet. Start one above.</p>
        ) : (
          <ul>
            {past.map((s) => (
              <li key={s.id}>
                <span className="when">{new Date(s.started_at).toLocaleString()}</span>
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
