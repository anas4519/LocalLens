// Highlighting search words inside OCR text, safely: we split the text into plain strings and
// wrap matches in <mark> React elements. No HTML string is ever built, so OCR text can't inject markup.

/**
 * Words of a query, roughly the way SQLite's unicode61 tokenizer sees them: lowercased and split on
 * anything that isn't a letter or digit. `"maxAttempts:"` -> `["maxattempts"]`.
 * Longest first, so "retry" wins over "re" when both could match at the same spot.
 */
export function searchTerms(query: string): string[] {
  const words = query.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  return [...new Set(words)].sort((a, b) => b.length - a.length);
}

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * Case-insensitive match at the start of a word, like FTS5's prefix query: "deplo" lights up
 * "deploy" and "redeploy" stays dark. The lookbehind means "not preceded by a letter or digit".
 */
function termRegex(terms: string[]): RegExp | null {
  return terms.length ? new RegExp(`(?<![\\p{L}\\p{N}])(?:${terms.map(escapeRe).join("|")})`, "giu") : null;
}

export interface Part {
  text: string;
  hit: boolean;
}

export function splitHits(text: string, terms: string[]): Part[] {
  const re = termRegex(terms);
  if (!re) return [{ text, hit: false }];
  const parts: Part[] = [];
  let last = 0;
  for (const m of text.matchAll(re)) {
    const i = m.index ?? 0;
    if (i > last) parts.push({ text: text.slice(last, i), hit: false });
    parts.push({ text: m[0], hit: true });
    last = i + m[0].length;
  }
  if (last < text.length) parts.push({ text: text.slice(last), hit: false });
  return parts;
}

export const countHits = (text: string, terms: string[]) => splitHits(text, terms).filter((p) => p.hit).length;

/** Every term starts a word somewhere in the text (the in-session filter's AND semantics, like FTS5). */
export function matchesAll(text: string, terms: string[]): boolean {
  return terms.every((w) => termRegex([w])!.test(text));
}

/** Render parts as text nodes and <mark>s. Works for our own splits and for backend snippets alike. */
export function Marked({ parts, markClass }: { parts: Part[]; markClass: string }) {
  return (
    <>
      {parts.map((p, i) =>
        p.hit ? (
          <mark key={i} className={markClass}>
            {p.text}
          </mark>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
  );
}
