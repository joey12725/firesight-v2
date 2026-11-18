// A small Markdown parser for the assistant's answers. It produces a tree, never HTML:
// the renderer builds React elements from the tree, so nothing the model writes is ever
// interpreted as markup. Raw HTML in the source stays visible as text. Links are kept
// only when they are plain http(s) or mailto addresses.
//
// Supported: paragraphs, headings, bold, italic (asterisks only, so snake_case field
// names survive), inline code, fenced code, bulleted and numbered lists (nested),
// block quotes, rules, links and pipe tables. It tolerates input that stops mid-way,
// because answers are rendered while they stream.

export type Inline =
  | { t: 'text'; v: string }
  | { t: 'strong'; c: Inline[] }
  | { t: 'em'; c: Inline[] }
  | { t: 'code'; v: string }
  | { t: 'link'; href: string; c: Inline[] };

export type Align = 'left' | 'right' | 'center' | null;

export type Block =
  | { t: 'p'; c: Inline[] }
  | { t: 'h'; level: 1 | 2 | 3 | 4; c: Inline[] }
  | { t: 'list'; ordered: boolean; start: number; items: Block[][] }
  | { t: 'code'; lang: string; v: string }
  | { t: 'quote'; c: Block[] }
  | { t: 'hr' }
  | { t: 'table'; align: Align[]; head: Inline[][]; rows: Inline[][][] };

/** The address if it is safe to make clickable, else null. */
export function safeHref(raw: string): string | null {
  const href = raw.trim();
  // No whitespace or control characters anywhere in the address.
  for (const ch of href) if (ch.charCodeAt(0) <= 0x20) return null;
  try {
    const url = new URL(href);
    return url.protocol === 'https:' || url.protocol === 'http:' || url.protocol === 'mailto:' ? url.href : null;
  } catch {
    return null;
  }
}

function pushText(out: Inline[], text: string) {
  if (!text) return;
  const last = out[out.length - 1];
  if (last?.t === 'text') last.v += text;
  else out.push({ t: 'text', v: text });
}

export function parseInline(source: string, depth = 0): Inline[] {
  const out: Inline[] = [];
  let i = 0;
  let plain = '';
  const flush = () => {
    pushText(out, plain);
    plain = '';
  };
  while (i < source.length) {
    const ch = source[i] as string;
    if (ch === '\\' && i + 1 < source.length && /[\\`*_[\]()#|>~-]/.test(source[i + 1] as string)) {
      plain += source[i + 1];
      i += 2;
      continue;
    }
    if (ch === '`') {
      const run = /^`+/.exec(source.slice(i))?.[0] ?? '`';
      const end = source.indexOf(run, i + run.length);
      if (end > 0) {
        flush();
        out.push({ t: 'code', v: source.slice(i + run.length, end).trim() });
        i = end + run.length;
        continue;
      }
    }
    if (ch === '*' && depth < 4) {
      const strong = source.startsWith('**', i);
      const marker = strong ? '**' : '*';
      const from = i + marker.length;
      const end = source.indexOf(marker, from);
      const inner = end > 0 ? source.slice(from, end) : '';
      if (end > from && !/^\s|\s$/.test(inner)) {
        flush();
        out.push({ t: strong ? 'strong' : 'em', c: parseInline(inner, depth + 1) });
        i = end + marker.length;
        continue;
      }
    }
    if (ch === '[' && depth < 4) {
      const close = source.indexOf('](', i);
      // The address may itself contain balanced parentheses.
      let end = -1;
      for (let j = close + 2, open = 0; close > 0 && j < source.length; j++) {
        const c = source[j];
        if (c === '\n') break;
        if (c === '(') open += 1;
        else if (c === ')') {
          if (open === 0) {
            end = j;
            break;
          }
          open -= 1;
        }
      }
      if (close > i && end > 0 && !source.slice(i + 1, close).includes('\n')) {
        const label = source.slice(i + 1, close);
        const href = safeHref(source.slice(close + 2, end).split(/\s+/)[0] ?? '');
        flush();
        if (href) out.push({ t: 'link', href, c: parseInline(label, depth + 1) });
        else out.push(...parseInline(label, depth + 1));
        i = end + 1;
        continue;
      }
    }
    plain += ch;
    i += 1;
  }
  flush();
  return out;
}

const FENCE = /^\s{0,3}(`{3,}|~{3,})\s*([\w+-]*)\s*$/;
const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const TABLE_RULE = /^\s*\|?\s*:?-{1,}:?\s*(\|\s*:?-{1,}:?\s*)*\|?\s*$/;

function cells(line: string): string[] {
  let body = line.trim();
  if (body.startsWith('|')) body = body.slice(1);
  if (body.endsWith('|') && !body.endsWith('\\|')) body = body.slice(0, -1);
  const out: string[] = [];
  let cell = '';
  let code = false;
  for (let i = 0; i < body.length; i++) {
    const ch = body[i] as string;
    if (ch === '`') code = !code;
    if (ch === '\\' && body[i + 1] === '|') {
      cell += '|';
      i += 1;
    } else if (ch === '|' && !code) {
      out.push(cell.trim());
      cell = '';
    } else cell += ch;
  }
  out.push(cell.trim());
  return out;
}

export function parseMarkdown(source: string, depth = 0): Block[] {
  const lines = source.replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let i = 0;
  const blank = (line: string | undefined) => line === undefined || line.trim() === '';

  while (i < lines.length) {
    const line = lines[i] as string;
    if (blank(line)) {
      i += 1;
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      const marker = fence[1] as string;
      const body: string[] = [];
      i += 1;
      while (i < lines.length && !(lines[i] as string).trim().startsWith(marker)) body.push(lines[i++] as string);
      i += 1;
      blocks.push({ t: 'code', lang: fence[2] ?? '', v: body.join('\n') });
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      blocks.push({ t: 'h', level: Math.min(4, (heading[1] as string).length) as 1 | 2 | 3 | 4, c: parseInline(heading[2] ?? '') });
      i += 1;
      continue;
    }

    if (RULE.test(line)) {
      blocks.push({ t: 'hr' });
      i += 1;
      continue;
    }

    if (line.includes('|') && TABLE_RULE.test(lines[i + 1] ?? '') && (lines[i + 1] ?? '').includes('-')) {
      const head = cells(line);
      const align = cells(lines[i + 1] as string).map((c): Align => (c.startsWith(':') && c.endsWith(':') ? 'center' : c.endsWith(':') ? 'right' : c.startsWith(':') ? 'left' : null));
      const rows: Inline[][][] = [];
      i += 2;
      while (i < lines.length && !blank(lines[i]) && (lines[i] as string).includes('|')) {
        const row = cells(lines[i++] as string);
        rows.push(head.map((_, col) => parseInline(row[col] ?? '')));
      }
      blocks.push({ t: 'table', align: head.map((_, col) => align[col] ?? null), head: head.map((c) => parseInline(c)), rows });
      continue;
    }

    if (/^\s{0,3}>/.test(line) && depth < 4) {
      const body: string[] = [];
      while (i < lines.length && /^\s{0,3}>/.test(lines[i] as string)) body.push((lines[i++] as string).replace(/^\s{0,3}>\s?/, ''));
      blocks.push({ t: 'quote', c: parseMarkdown(body.join('\n'), depth + 1) });
      continue;
    }

    const item = ITEM.exec(line);
    if (item && depth < 4) {
      const indent = (item[1] as string).length;
      const ordered = /\d/.test(item[2] as string);
      const items: string[][] = [];
      while (i < lines.length) {
        const current = lines[i] as string;
        const next = ITEM.exec(current);
        if (next && (next[1] as string).length <= indent + 1 && /\d/.test(next[2] as string) === ordered) {
          items.push([next[3] ?? '']);
          i += 1;
        } else if (blank(current)) {
          // A blank line ends the list unless the next line continues it.
          const after = lines[i + 1];
          if (after !== undefined && (/^\s{2,}\S/.test(after) || (ITEM.exec(after) && /\d/.test(ITEM.exec(after)?.[2] ?? '') === ordered))) {
            (items[items.length - 1] as string[]).push('');
            i += 1;
          } else break;
        } else if (/^\s{2,}\S/.test(current) || (!ITEM.test(current) && !HEADING.test(current) && !FENCE.test(current))) {
          (items[items.length - 1] as string[]).push(current.replace(new RegExp(`^\\s{0,${indent + 4}}`), ''));
          i += 1;
        } else break;
      }
      blocks.push({ t: 'list', ordered, start: ordered ? Number.parseInt(item[2] as string, 10) || 1 : 1, items: items.map((body) => parseMarkdown(body.join('\n'), depth + 1)) });
      continue;
    }

    const paragraph: string[] = [];
    while (i < lines.length && !blank(lines[i])) {
      const current = lines[i] as string;
      if (paragraph.length > 0 && (FENCE.test(current) || HEADING.test(current) || RULE.test(current) || ITEM.test(current) || /^\s{0,3}>/.test(current))) break;
      if (paragraph.length > 0 && current.includes('|') && TABLE_RULE.test(lines[i + 1] ?? '') && (lines[i + 1] ?? '').includes('-')) break;
      paragraph.push(current.trim());
      i += 1;
    }
    blocks.push({ t: 'p', c: parseInline(paragraph.join('\n')) });
  }
  return blocks;
}
