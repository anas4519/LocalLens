import { getVersion } from "@tauri-apps/api/app";
import { useCallback, useEffect, useState } from "react";
import Gallery from "./Gallery";
import Search from "./Search";
import Sessions from "./Sessions";
import { SearchHit, Session, captureStatus, listSessions, startSession, stopSession } from "./api";
import appIcon from "./assets/app-icon.png";
import { plural } from "./format";
import { Icon, RecDot, isTyping } from "./ui";

type View = "sessions" | "search";

/** An open gallery remembers where it was opened from, so Back / Esc returns there. */
interface Viewing {
  session: Session;
  frameId: number | null; // jump the lightbox to this frame (from a search hit)
  from: View;
}

const navBase = "group flex items-center justify-between px-space-sm py-space-sm rounded-lg transition-colors";
const navActive = "bg-surface-container-high text-primary font-medium";
const navIdle = "text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface";
const navKbd =
  "px-1.5 py-0.5 rounded bg-surface-container font-shortcut-key text-shortcut-key text-outline group-hover:text-on-surface-variant";

function NavItem({
  icon,
  label,
  hint,
  active = false,
  disabled = false,
  onClick,
}: {
  icon: string;
  label: string;
  hint: string;
  active?: boolean;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      aria-current={active ? "page" : undefined}
      className={`${navBase} ${active ? navActive : navIdle} ${disabled ? "opacity-40 cursor-not-allowed hover:bg-transparent" : ""}`}
      onClick={disabled ? undefined : onClick}
      disabled={disabled}
      title={disabled ? "Coming soon" : undefined}
    >
      <div className="flex items-center gap-space-sm">
        <Icon name={icon} className="text-[18px]" />
        <span className="font-body-md text-body-md">{label}</span>
      </div>
      <kbd className={navKbd}>{hint}</kbd>
    </button>
  );
}

export default function App() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [active, setActive] = useState<Session | null>(null);
  const [now, setNow] = useState(Date.now());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [hasPermission, setHasPermission] = useState(true);
  const [captureError, setCaptureError] = useState<string | null>(null);
  const [ocrPending, setOcrPending] = useState(0);
  const [newName, setNewName] = useState("");
  const [view, setView] = useState<View>("sessions");
  const [viewing, setViewing] = useState<Viewing | null>(null);
  const [query, setQuery] = useState("");
  const [focusSearch, setFocusSearch] = useState(0);
  const [version, setVersion] = useState<string | null>(null);

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
    getVersion().then(setVersion, () => {});
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

  // One place to apply a rename result, wherever it came from (list, gallery, live banner).
  const applyRename = (s: Session) => {
    setSessions((all) => all.map((x) => (x.id === s.id ? s : x)));
    setActive((a) => (a && a.id === s.id ? s : a));
    setViewing((v) => (v && v.session.id === s.id ? { ...v, session: s } : v));
  };

  const go = (v: View) => {
    setViewing(null);
    setView(v);
    if (v === "search") setFocusSearch((n) => n + 1);
  };

  // A search hit only carries the session id; get the full session (refetch if it's new).
  const openHit = async (h: SearchHit, jump: boolean) => {
    try {
      let s = sessions.find((x) => x.id === h.session_id);
      if (!s) {
        const all = await listSessions();
        setSessions(all);
        s = all.find((x) => x.id === h.session_id);
      }
      if (!s) return setError("That session no longer exists.");
      setViewing({ session: s, frameId: jump ? h.frame_id : null, from: "search" });
    } catch (e) {
      setError(String(e));
    }
  };

  // Global shortcuts: ⌘K or "/" -> Search, ⌘1 -> Sessions.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        go("search");
      } else if (mod && e.key === "1") {
        e.preventDefault();
        go("sessions");
      } else if (e.key === "/" && !mod && !isTyping(e.target)) {
        e.preventDefault();
        go("search");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const totalFrames = sessions.reduce((n, s) => n + s.frame_count, 0);
  const current: View = viewing ? viewing.from : view;

  return (
    <>
      <aside className="fixed left-0 top-0 h-screen w-60 bg-surface-container-lowest z-50 flex flex-col justify-between shadow-[1px_0_0_0_rgba(255,255,255,0.06)]">
        <div className="flex flex-col">
          {/* Native macOS traffic lights sit here (titleBarStyle: Overlay); the strip drags the window. */}
          <div className="h-14 px-space-lg flex items-center gap-space-sm pt-space-xs" data-tauri-drag-region />
          <div className="px-space-md pb-space-md flex items-center justify-between">
            <div className="flex items-center gap-space-sm">
              <img alt="LocalLens App Icon" className="h-8 w-auto object-contain" src={appIcon} />
              <div className="flex flex-col">
                <span className="font-headline-sm text-headline-sm font-semibold tracking-tight text-on-surface">
                  LocalLens
                </span>
                <span className="font-label-sm text-label-sm text-outline tracking-wider">
                  {version ? `v${version} · ` : ""}on-device
                </span>
              </div>
            </div>
          </div>
          <div className="px-space-md py-space-xs">
            <div className="px-space-sm py-1 font-label-sm text-label-sm text-outline uppercase tracking-wider">
              Navigation
            </div>
            <nav className="flex flex-col gap-1 mt-1">
              <NavItem
                icon="photo_library"
                label="Sessions"
                hint="⌘1"
                active={current === "sessions"}
                onClick={() => go("sessions")}
              />
              <NavItem
                icon="manage_search"
                label="Search"
                hint="⌘K"
                active={current === "search"}
                onClick={() => go("search")}
              />
              <NavItem icon="chat_bubble" label="Ask" hint="Soon" disabled />
              <NavItem icon="settings" label="Settings" hint="Soon" disabled />
            </nav>
          </div>
        </div>
        <div className="p-space-md flex flex-col gap-space-md">
          <div className="p-space-sm rounded-xl bg-surface-container-low/60 flex flex-col gap-space-xs">
            <div className="flex items-center justify-between text-on-surface-variant">
              <span className="font-label-sm text-label-sm">LOCAL INDEX</span>
              <span className="font-label-sm text-label-sm text-outline">{plural(totalFrames, "frame")}</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="font-label-sm text-label-sm text-on-surface font-medium">
                {plural(sessions.length, "session")}
              </span>
              <span className="font-label-sm text-label-sm text-outline">SQLite · this Mac</span>
            </div>
          </div>
          <div className="p-space-sm rounded-xl bg-surface-container-low/40 flex items-center gap-space-sm">
            <Icon name="shield_lock" className="text-[16px] text-tertiary" />
            <div className="flex flex-col">
              <span className="font-body-sm text-body-sm font-medium text-on-surface">100% On-Device</span>
              <span className="font-label-sm text-label-sm text-outline">No data leaves your Mac</span>
            </div>
          </div>
        </div>
      </aside>

      <div className="pl-60">
        <header
          className="fixed top-0 left-60 right-0 h-14 bg-surface-container-lowest/80 backdrop-blur-xl z-40 flex items-center justify-between px-space-xl shadow-[0_1px_0_0_rgba(255,255,255,0.04)]"
          data-tauri-drag-region
        >
          <div className="flex items-center gap-space-md flex-1 max-w-lg">
            <button
              className="w-full flex items-center gap-space-sm px-space-md py-1.5 rounded-lg bg-surface-container text-outline hover:text-on-surface-variant cursor-pointer transition-colors shadow-[0_1px_4px_rgba(0,0,0,0.2)]"
              onClick={() => go("search")}
            >
              <Icon name="search" className="text-[16px]" />
              <span className="font-body-md text-body-md flex-1 text-left text-outline truncate">
                {query.trim() && current === "search" ? query : "Search indexed screen history and OCR text..."}
              </span>
              <kbd className="px-1.5 py-0.5 rounded bg-surface-container-high font-shortcut-key text-shortcut-key text-outline">
                ⌘K
              </kbd>
            </button>
          </div>
          <div className="flex items-center gap-space-md">
            {active ? (
              <button
                className="flex items-center gap-space-sm px-space-sm py-1 rounded-full bg-surface-container text-on-surface-variant"
                onClick={() => go("sessions")}
                title="Recording: go to the live session"
              >
                <RecDot />
                <span className="font-label-sm text-label-sm tracking-wider uppercase text-on-surface">REC · ACTIVE</span>
              </button>
            ) : (
              <div className="flex items-center gap-space-sm px-space-sm py-1 rounded-full bg-surface-container text-on-surface-variant">
                <span className="relative inline-flex rounded-full h-2 w-2 bg-outline-variant" />
                <span className="font-label-sm text-label-sm tracking-wider uppercase text-outline">IDLE</span>
              </div>
            )}
          </div>
        </header>

        <main className="w-full pt-14 min-h-screen bg-surface">
          {viewing ? (
            <Gallery
              key={viewing.session.id}
              session={viewing.session}
              initialFrameId={viewing.frameId}
              highlight={viewing.from === "search" ? query : ""}
              backLabel={viewing.from === "search" ? "Search Results" : "All Sessions"}
              onBack={() => setViewing(null)}
              onRenamed={applyRename}
            />
          ) : view === "search" ? (
            <Search
              query={query}
              onQueryChange={setQuery}
              onOpen={(h) => openHit(h, true)}
              onOpenSession={(h) => openHit(h, false)}
              focusSignal={focusSearch}
            />
          ) : (
            <Sessions
              sessions={sessions}
              active={active}
              now={now}
              ocrPending={ocrPending}
              hasPermission={hasPermission}
              captureError={captureError}
              error={error}
              busy={busy}
              newName={newName}
              onNewName={setNewName}
              onToggle={toggle}
              onOpen={(s) => setViewing({ session: s, frameId: null, from: "sessions" })}
              onRenamed={applyRename}
              version={version}
            />
          )}
        </main>
      </div>
    </>
  );
}
