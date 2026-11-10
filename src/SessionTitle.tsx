import { useEffect, useRef, useState } from "react";
import { MAX_SESSION_NAME, Session, renameSession } from "./api";

/** "Tue, 10 Nov · 7:55 PM": the date + time every session shows, named or not. */
export const fmtWhen = (ms: number) =>
  new Date(ms).toLocaleString([], {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });

interface Props {
  session: Session;
  onRenamed: (s: Session) => void;
  /** "h1" in the gallery header, "span" inside list rows. */
  as?: "h1" | "span";
}

/**
 * Named:   name (title) + date/time (muted, beneath).
 * Unnamed: date/time (title).
 * The pencil swaps the title for an input: Enter saves, Esc cancels, blank clears the name.
 */
export default function SessionTitle({ session, onRenamed, as: Tag = "span" }: Props) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const when = fmtWhen(session.started_at);

  useEffect(() => {
    if (editing) input.current?.select();
  }, [editing]);

  const begin = (e: React.MouseEvent) => {
    e.stopPropagation(); // the list row behind is clickable
    setDraft(session.name ?? "");
    setError(null);
    setEditing(true);
  };

  const save = async () => {
    if (saving) return;
    const next = draft.trim();
    if (next === (session.name ?? "")) return setEditing(false);
    setSaving(true);
    try {
      onRenamed(await renameSession(session.id, next || null));
      setEditing(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  if (editing) {
    return (
      <div className="session-title editing" onClick={(e) => e.stopPropagation()}>
        <input
          ref={input}
          className="name-input"
          value={draft}
          maxLength={MAX_SESSION_NAME}
          placeholder="Session name (blank = date + time)"
          disabled={saving}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation(); // keep Esc from also closing the gallery
            if (e.key === "Enter") save();
            else if (e.key === "Escape") setEditing(false);
          }}
          onBlur={() => !saving && setEditing(false)}
          aria-label="Session name"
        />
        <span className="when-sub">{error ?? `Enter to save · Esc to cancel · ${when}`}</span>
      </div>
    );
  }

  return (
    <div className="session-title">
      <div className="title-row">
        <Tag className={`title ${Tag === "h1" ? "large" : ""}`}>{session.name ?? when}</Tag>
        <button className="icon-btn" onClick={begin} aria-label="Rename session" title="Rename">
          <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden>
            <path
              fill="none"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M4 20h4L19 9l-4-4L4 16v4zM13.5 6.5l4 4"
            />
          </svg>
        </button>
      </div>
      {session.name && <span className="when-sub">{when}</span>}
    </div>
  );
}
