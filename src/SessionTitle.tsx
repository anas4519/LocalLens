import { useEffect, useRef, useState } from "react";
import { MAX_SESSION_NAME, Session, renameSession } from "./api";
import { fmtWhen } from "./format";
import { Icon } from "./ui";

/** Where the title sits; each place uses the Stitch classes from its own screen. */
type Variant = "live" | "card" | "header";

const STYLE: Record<Variant, { named: string; unnamed: string; pencil: string; icon: string; input: string }> = {
  // Sessions screen, recording banner.
  live: {
    named: "font-headline-lg text-headline-lg tracking-tight text-on-surface font-semibold truncate max-w-xl",
    unnamed: "font-headline-lg text-headline-lg tracking-tight text-on-surface font-semibold truncate max-w-xl",
    pencil: "p-1 rounded-lg text-outline hover:text-on-surface hover:bg-surface-container transition-colors",
    icon: "text-[16px]",
    input: "font-headline-md text-headline-md",
  },
  // Sessions screen, history row (the row is the hover `group`).
  card: {
    named:
      "font-headline-sm text-headline-sm font-medium text-on-surface group-hover:text-primary transition-colors truncate",
    unnamed:
      "font-headline-sm text-headline-sm font-medium text-on-surface-variant group-hover:text-on-surface transition-colors italic truncate",
    pencil:
      "opacity-0 group-hover:opacity-100 focus:opacity-100 p-0.5 hover:text-on-surface text-outline transition-opacity",
    icon: "text-[14px]",
    input: "font-headline-sm text-headline-sm",
  },
  // Gallery header.
  header: {
    named: "font-headline-md text-headline-md font-semibold text-on-surface truncate",
    unnamed: "font-headline-md text-headline-md font-semibold text-on-surface truncate",
    pencil: "p-1 rounded-lg text-outline hover:text-on-surface hover:bg-surface-container-high transition-colors",
    icon: "text-[16px]",
    input: "font-headline-md text-headline-md",
  },
};

interface Props {
  session: Session;
  onRenamed: (s: Session) => void;
  variant: Variant;
  as?: "h1" | "h2" | "h3";
  /** Bump this number to open the rename box from outside (the R shortcut on the sessions list). */
  editSignal?: number;
}

/**
 * Title = the session's name, or its date + time when unnamed (with an "Unnamed" tag).
 * The pencil swaps the title for an input: Enter saves, Esc cancels, blank clears the name.
 * Where the date shows under a named title is up to the caller (each screen lays it out differently).
 */
export default function SessionTitle({ session, onRenamed, variant, as: Tag = "h3", editSignal }: Props) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const st = STYLE[variant];

  const begin = () => {
    setDraft(session.name ?? "");
    setError(null);
    setEditing(true);
  };

  useEffect(() => {
    if (editSignal) begin();
    // Only react to a new signal, not to the session object changing.
  }, [editSignal]);

  useEffect(() => {
    if (editing) input.current?.select();
  }, [editing]);

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
      <div className="flex flex-col gap-1 min-w-0 w-full max-w-xl" onClick={(e) => e.stopPropagation()}>
        <input
          ref={input}
          className={`${st.input} w-full bg-surface-container-lowest text-on-surface placeholder:text-outline rounded-lg px-space-sm py-1 shadow-[0_0_0_1px_rgba(252,211,77,0.4),0_0_18px_rgba(245,158,11,0.18)] focus:outline-none select-text`}
          value={draft}
          maxLength={MAX_SESSION_NAME}
          placeholder="Session name (blank = date + time)"
          disabled={saving}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation(); // keep Esc/arrows/R from also reaching the screen's shortcuts
            if (e.key === "Enter") save();
            else if (e.key === "Escape") setEditing(false);
          }}
          onBlur={() => !saving && setEditing(false)}
          aria-label="Session name"
        />
        <span className={`font-label-sm text-label-sm ${error ? "text-error" : "text-outline"}`}>
          {error ?? `Enter to save · Esc to cancel · ${fmtWhen(session.started_at)}`}
        </span>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-space-sm min-w-0">
      <Tag className={session.name ? st.named : st.unnamed}>{session.name ?? fmtWhen(session.started_at)}</Tag>
      {!session.name && variant === "card" && (
        <span className="font-label-sm text-label-sm px-1.5 py-0.5 rounded bg-surface-container-lowest text-outline shrink-0">
          Unnamed
        </span>
      )}
      <button
        className={`${st.pencil} shrink-0 flex items-center`}
        onClick={(e) => {
          e.stopPropagation(); // the card behind is clickable
          begin();
        }}
        title={session.name ? "Rename session" : "Set title"}
        aria-label="Rename session"
      >
        <Icon name="edit" className={st.icon} />
      </button>
    </div>
  );
}
