import { memo, useMemo, type ReactNode } from 'react';
import { cx } from '@/ui';
import { parseMarkdown, type Block, type Inline } from '../markdown';

function inline(nodes: Inline[]): ReactNode[] {
  return nodes.map((node, i) => {
    switch (node.t) {
      case 'text':
        return node.v;
      case 'strong':
        return (
          <strong key={i} className="font-semibold text-ink-1">
            {inline(node.c)}
          </strong>
        );
      case 'em':
        return <em key={i}>{inline(node.c)}</em>;
      case 'code':
        return (
          <code key={i} className="num rounded-xs bg-surface-3 px-1 py-px text-[0.92em]">
            {node.v}
          </code>
        );
      case 'link':
        return (
          <a key={i} href={node.href} target="_blank" rel="noopener noreferrer nofollow" className="text-accent-ink underline decoration-line-strong underline-offset-2 hover:decoration-accent">
            {inline(node.c)}
          </a>
        );
    }
  });
}

const HEADING = ['mt-4 text-base font-semibold tracking-tight', 'mt-4 text-base font-semibold tracking-tight', 'mt-3 text-sm font-semibold', 'mt-3 text-sm font-medium text-ink-2'];
const ALIGN = { left: 'text-left', right: 'text-right', center: 'text-center' } as const;

/** A cell that is only a figure reads better in tabular numerals. */
const numeric = (cell: Inline[]) => cell.length === 1 && cell[0]?.t === 'text' && /^[\s$€£~<>+−-]*\d[\d,.\s]*(%|k|m|bn|yrs?|years?)?$/i.test(cell[0].v.trim());

function blocks(nodes: Block[], tight = false): ReactNode[] {
  return nodes.map((node, i) => {
    switch (node.t) {
      case 'p':
        return (
          <p key={i} className={cx('leading-relaxed text-pretty', !tight && 'mt-2 first:mt-0')}>
            {inline(node.c)}
          </p>
        );
      case 'h': {
        const Tag = (['h3', 'h3', 'h4', 'h5'] as const)[node.level - 1] ?? 'h5';
        return (
          <Tag key={i} className={cx(HEADING[node.level - 1], 'first:mt-0')}>
            {inline(node.c)}
          </Tag>
        );
      }
      case 'list': {
        const Tag = node.ordered ? 'ol' : 'ul';
        return (
          <Tag key={i} start={node.ordered && node.start !== 1 ? node.start : undefined} className={cx('mt-2 space-y-1 pl-5 first:mt-0', node.ordered ? 'list-decimal' : 'list-disc', 'marker:text-ink-3')}>
            {node.items.map((item, j) => (
              <li key={j} className="pl-0.5">
                {blocks(item, true)}
              </li>
            ))}
          </Tag>
        );
      }
      case 'code':
        return (
          <pre key={i} className="scroll-thin mt-2 overflow-x-auto rounded-sm border border-line bg-surface-2 p-2.5 text-xs leading-relaxed first:mt-0">
            <code className="num">{node.v}</code>
          </pre>
        );
      case 'quote':
        return (
          <blockquote key={i} className="mt-2 border-l-2 border-line-strong pl-3 text-ink-2 first:mt-0">
            {blocks(node.c)}
          </blockquote>
        );
      case 'hr':
        return <hr key={i} className="my-3 border-line" />;
      case 'table':
        return (
          <div key={i} className="scroll-thin mt-2 overflow-x-auto first:mt-0" tabIndex={0} role="group" aria-label="Table">
            <table className="w-full border-collapse text-xs">
              <thead>
                <tr>
                  {node.head.map((cell, c) => (
                    <th key={c} scope="col" className={cx('border-b border-line-strong px-2 py-1.5 align-bottom font-medium text-ink-2 first:pl-0 last:pr-0', ALIGN[node.align[c] ?? 'left'])}>
                      {inline(cell)}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {node.rows.map((row, r) => (
                  <tr key={r}>
                    {row.map((cell, c) => (
                      <td key={c} className={cx('border-b border-line px-2 py-1.5 align-baseline first:pl-0 last:pr-0', ALIGN[node.align[c] ?? (numeric(cell) && c > 0 ? 'right' : 'left')], numeric(cell) && 'num whitespace-nowrap')}>
                        {inline(cell)}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        );
    }
  });
}

/** Renders the assistant's Markdown. No HTML from the source is ever interpreted. */
export const Markdown = memo(function Markdown({ source, className }: { source: string; className?: string }) {
  const tree = useMemo(() => parseMarkdown(source), [source]);
  return <div className={cx('min-w-0 text-sm text-ink-1 [overflow-wrap:anywhere]', className)}>{blocks(tree)}</div>;
});
