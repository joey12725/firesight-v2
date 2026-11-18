import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { splitFollowups } from './followups';
import { parseInline, parseMarkdown, safeHref, type Block, type Inline } from './markdown';
import { Markdown } from './ui/Markdown';

const html = (source: string) => renderToStaticMarkup(createElement(Markdown, { source }));

/** Every element name in the rendered output. */
const tags = (markup: string) => [...new Set([...markup.matchAll(/<([a-z0-9]+)/g)].map((m) => m[1]))].sort();

describe('markdown parser', () => {
  it('parses the subset answers use', () => {
    const tree = parseMarkdown(['## Result', '', 'In **about 7 of 100** lives the plan runs *short* (`depletion`).', '', '- one', '- two', '  - nested', '', '1. first', '2. second', '', '> a caveat', '', '---', '', '```json', '{"a": 1}', '```'].join('\n'));
    expect(tree.map((b) => b.t)).toEqual(['h', 'p', 'list', 'list', 'quote', 'hr', 'code']);
    const p = tree[1] as Extract<Block, { t: 'p' }>;
    expect(p.c.map((n) => n.t)).toEqual(['text', 'strong', 'text', 'em', 'text', 'code', 'text']);
    const list = tree[2] as Extract<Block, { t: 'list' }>;
    expect(list.items).toHaveLength(2);
    expect(list.items[1]!.map((b) => b.t)).toEqual(['p', 'list']);
    expect((tree[3] as Extract<Block, { t: 'list' }>).ordered).toBe(true);
    expect(tree[6]).toEqual({ t: 'code', lang: 'json', v: '{"a": 1}' });
  });

  it('parses tables with alignment', () => {
    const tree = parseMarkdown(['| Option | Success | Median FI age |', '|:--|--:|:-:|', '| Plan | 91% | 52 |', '| Retire at 55 | 84% | 55 |'].join('\n'));
    const table = tree[0] as Extract<Block, { t: 'table' }>;
    expect(table.t).toBe('table');
    expect(table.align).toEqual(['left', 'right', 'center']);
    expect(table.rows).toHaveLength(2);
    expect(table.rows[1]![0]).toEqual([{ t: 'text', v: 'Retire at 55' }]);
    const markup = html('| A | B |\n|---|---|\n| 1 | $2,000 |');
    expect(markup).toContain('<table');
    expect(markup).toContain('<th scope="col"');
  });

  it('leaves snake_case and stray asterisks alone', () => {
    expect(parseInline('success_rate and median_fi_age')).toEqual([{ t: 'text', v: 'success_rate and median_fi_age' }]);
    expect(parseInline('5 * 3 = 15')).toEqual([{ t: 'text', v: '5 * 3 = 15' }]);
    expect(parseInline('a \\*literal\\* star')).toEqual([{ t: 'text', v: 'a *literal* star' }]);
  });

  it('copes with text that stops mid-stream', () => {
    expect(parseMarkdown('```json\n{"a":').at(-1)).toEqual({ t: 'code', lang: 'json', v: '{"a":' });
    expect(() => parseMarkdown('| A | B |\n|--')).not.toThrow();
    expect(() => parseMarkdown('**bold never closes and [a link](http')).not.toThrow();
    expect(() => parseMarkdown('- item\n  - ')).not.toThrow();
  });
});

describe('markdown safety', () => {
  it('only links to http(s) and mailto addresses', () => {
    expect(safeHref('https://example.com/a?b=1')).toBe('https://example.com/a?b=1');
    expect(safeHref('mailto:someone@example.com')).toBe('mailto:someone@example.com');
    for (const bad of ['javascript:alert(1)', 'JaVaScRiPt:alert(1)', ' javascript:alert(1)', 'java\tscript:alert(1)', 'data:text/html,<script>alert(1)</script>', 'vbscript:x', 'file:///etc/passwd', '//evil.example', '/relative', 'blob:https://x']) {
      expect(safeHref(bad), bad).toBeNull();
    }
    const links = (nodes: Inline[]): string[] => nodes.flatMap((n) => (n.t === 'link' ? [n.href] : 'c' in n ? links(n.c) : []));
    expect(links(parseInline('[click](javascript:alert(1)) and [ok](https://ok.example) and **[nested](data:text/html,x)**'))).toEqual(['https://ok.example/']);
  });

  it('never turns source text into markup', () => {
    const hostile = [
      '<script>alert(1)</script>',
      '<img src=x onerror="alert(1)">',
      '<iframe src="https://evil.example"></iframe>',
      '<a href="javascript:alert(1)">x</a>',
      '[x](javascript:alert(1))',
      '![img](https://evil.example/pixel.png)',
      '<svg/onload=alert(1)>',
      '`<b>in code</b>`',
      '```html\n<script>alert(2)</script>\n```',
      '| <script>x</script> | b |\n|---|---|\n| <img src=x> | 1 |',
      '<style>body{display:none}</style>',
      '&lt;script&gt;alert(3)&lt;/script&gt;',
    ].join('\n\n');
    const markup = html(hostile);
    // Only the elements the renderer itself creates appear; everything else is text.
    expect(tags(markup)).toEqual(['a', 'code', 'div', 'p', 'pre', 'table', 'tbody', 'td', 'th', 'thead', 'tr'].filter((t) => markup.includes(`<${t}`)));
    for (const forbidden of ['<script', '<img', '<iframe', '<svg', '<style', 'onerror=', 'onload=', 'javascript:']) {
      // Escaped text may mention them; live markup may not.
      expect(markup.replace(/&lt;[^]*?&gt;/g, '').replace(/&quot;/g, ''), forbidden).not.toContain(forbidden);
    }
    expect(markup).toContain('&lt;script&gt;alert(1)&lt;/script&gt;');
    expect(markup).not.toMatch(/<a [^>]*href="(?!https?:|mailto:)/);
  });

  it('opens links safely', () => {
    const markup = html('See [the IRS page](https://www.irs.gov/x).');
    expect(markup).toContain('href="https://www.irs.gov/x"');
    expect(markup).toContain('rel="noopener noreferrer nofollow"');
    expect(markup).toContain('target="_blank"');
  });
});

describe('follow-up suggestions', () => {
  it('splits the block from the answer', () => {
    const answer = 'The plan holds in about 91 of 100 lives.\n\n<followups>\n- What if I retire at 55?\n- How much could we spend?\n* Third\n- Fourth is dropped\n</followups>';
    expect(splitFollowups(answer)).toEqual({ body: 'The plan holds in about 91 of 100 lives.', followups: ['What if I retire at 55?', 'How much could we spend?', 'Third'] });
    expect(splitFollowups('No block here.')).toEqual({ body: 'No block here.', followups: [] });
  });

  it('hides the block while it is still streaming', () => {
    expect(splitFollowups('Answer.\n\n<follow').body).toBe('Answer.');
    expect(splitFollowups('Answer.\n\n<followups>\n- What if').body).toBe('Answer.');
    expect(splitFollowups('Compare a < b').body).toBe('Compare a < b');
    expect(splitFollowups('Compare a <').body).toBe('Compare a');
  });

  it('ignores markup smuggled into a suggestion', () => {
    expect(splitFollowups('x <followups>\n- <script>alert(1)</script>\n- fine\n</followups>').followups).toEqual(['fine']);
  });
});

describe('links with parentheses', () => {
  it('reads the whole address, safe or not', () => {
    expect(parseInline('[wiki](https://en.wikipedia.org/wiki/FIRE_(finance)) end')).toEqual([{ t: 'link', href: 'https://en.wikipedia.org/wiki/FIRE_(finance)', c: [{ t: 'text', v: 'wiki' }] }, { t: 'text', v: ' end' }]);
    expect(parseInline('[bad](javascript:alert(1)) end')).toEqual([{ t: 'text', v: 'bad end' }]);
  });
});
