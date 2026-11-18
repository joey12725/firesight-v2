// The model ends an answer with an optional <followups> block; the app shows the
// questions as buttons and never as text.

const OPEN = '<followups>';
const CLOSE = '</followups>';

export interface SplitAnswer {
  /** The answer without the block (and without a half-streamed opening tag). */
  body: string;
  followups: string[];
}

export function splitFollowups(text: string): SplitAnswer {
  const at = text.lastIndexOf(OPEN);
  if (at < 0) {
    // While streaming, the tail may be the first characters of the tag: hide them.
    for (let n = Math.min(OPEN.length - 1, text.length); n > 0; n--) {
      if (text.endsWith(OPEN.slice(0, n))) return { body: text.slice(0, -n).trimEnd(), followups: [] };
    }
    return { body: text, followups: [] };
  }
  const end = text.indexOf(CLOSE, at);
  const inner = text.slice(at + OPEN.length, end < 0 ? undefined : end);
  const followups = inner
    .split('\n')
    .map((line) => line.replace(/^\s*(?:[-*]|\d+[.)])\s*/, '').trim())
    .filter((line) => line.length > 0 && line.length <= 160 && !line.startsWith('<'))
    .slice(0, 3);
  const after = end < 0 ? '' : text.slice(end + CLOSE.length);
  return { body: (text.slice(0, at) + after).trimEnd(), followups };
}
