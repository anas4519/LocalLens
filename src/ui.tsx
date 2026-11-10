// Tiny shared building blocks, ported from repeated Stitch markup.

/** A Material Symbols glyph from the locally bundled font ("search", "edit", ...). Decorative only. */
export function Icon({ name, className = "" }: { name: string; className?: string }) {
  return (
    <span className={`material-symbols-outlined ${className}`} aria-hidden="true">
      {name}
    </span>
  );
}

/** Stitch's spinner SVG (used for "OCR pending"). */
export function Spinner({ className = "" }: { className?: string }) {
  return (
    <svg className={`animate-spin ${className}`} fill="none" viewBox="0 0 24 24" aria-hidden="true">
      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
      <path
        className="opacity-75"
        d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
        fill="currentColor"
      />
    </svg>
  );
}

/** The pulsing red recording beacon. */
export function RecDot() {
  return (
    <span className="relative flex h-2 w-2">
      <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-error opacity-75" />
      <span className="relative inline-flex rounded-full h-2 w-2 bg-error" />
    </span>
  );
}

/** True when a keypress is going into a text field, so single-key shortcuts should type, not act. */
export const isTyping = (t: EventTarget | null) =>
  t instanceof HTMLElement && (t.isContentEditable || t.tagName === "INPUT" || t.tagName === "TEXTAREA");

/** True when the user has selected some text, so ⌘C should do a normal copy instead of "copy all". */
export function hasTextSelection(): boolean {
  const a = document.activeElement;
  if (a instanceof HTMLInputElement || a instanceof HTMLTextAreaElement) {
    return (a.selectionStart ?? 0) !== (a.selectionEnd ?? 0);
  }
  return (window.getSelection()?.toString() ?? "") !== "";
}
