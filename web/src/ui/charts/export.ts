// Getting a chart out of the app: a PNG of what is drawn (canvas and SVG layers, with
// the title and takeaway above and the legend and caption with it) and a CSV of what
// is plotted.
import { csvValue } from './format';
import { getChartTheme } from './theme';

export interface TableColumn {
  key: string;
  label: string;
  /** Formats a cell for the on-screen table. The CSV always carries the raw value. */
  format?: (value: number) => string;
  align?: 'left' | 'right';
}

/** The data behind a chart, as rows: the table view and the CSV are both made from it. */
export interface TableData {
  /** Says what the rows are, for the table's caption. */
  caption: string;
  columns: TableColumn[];
  rows: Record<string, number | string | null>[];
}

export function tableToCsv(table: TableData): string {
  const head = table.columns.map((c) => csvValue(c.label)).join(',');
  const body = table.rows.map((row) => table.columns.map((c) => csvValue(row[c.key])).join(','));
  return [head, ...body].join('\n') + '\n';
}

export function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function wrap(ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string[] {
  const lines: string[] = [];
  let line = '';
  for (const word of text.split(/\s+/)) {
    const next = line ? `${line} ${word}` : word;
    if (line && ctx.measureText(next).width > maxWidth) {
      lines.push(line);
      line = word;
    } else line = next;
  }
  if (line) lines.push(line);
  return lines;
}

function svgImage(svg: SVGSVGElement, width: number, height: number): Promise<HTMLImageElement> {
  const clone = svg.cloneNode(true) as SVGSVGElement;
  clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
  clone.setAttribute('width', String(width));
  clone.setAttribute('height', String(height));
  clone.removeAttribute('style');
  // An SVG drawn as an image cannot load web fonts: name system fallbacks.
  clone.setAttribute('font-family', 'ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif');
  const url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(new XMLSerializer().serializeToString(clone))}`;
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error('The chart could not be drawn as an image.'));
    img.src = url;
  });
}

export interface ImageHeader {
  title?: string;
  takeaway?: string;
  caption?: string;
}

/**
 * Renders the chart layers inside `root` (every `[data-chart-layer]`, in document
 * order, at their on-screen positions) to a PNG at 2x, on the page surface, with the
 * title and takeaway above and the caption below.
 */
export async function chartToPng(root: HTMLElement, header: ImageHeader = {}): Promise<Blob> {
  const theme = getChartTheme();
  const scale = 2;
  const pad = 20;
  const box = root.getBoundingClientRect();
  const width = Math.max(320, Math.round(box.width)) + pad * 2;
  const font = (weight: number, size: number) => `${weight} ${size}px ${theme.fontSans}`;

  const measure = document.createElement('canvas').getContext('2d') as CanvasRenderingContext2D;
  const textWidth = width - pad * 2;
  measure.font = font(500, 15);
  const takeaway = header.takeaway ? wrap(measure, header.takeaway, textWidth) : [];
  measure.font = font(400, 11);
  const caption = header.caption ? wrap(measure, header.caption, textWidth) : [];
  const headerHeight = (header.title ? 20 : 0) + takeaway.length * 21 + (header.title || takeaway.length ? 10 : 0);
  const footerHeight = caption.length ? caption.length * 15 + 10 : 0;
  const height = pad + headerHeight + Math.round(box.height) + footerHeight + pad;

  const canvas = document.createElement('canvas');
  canvas.width = width * scale;
  canvas.height = height * scale;
  const ctx = canvas.getContext('2d') as CanvasRenderingContext2D;
  ctx.scale(scale, scale);
  ctx.fillStyle = theme.surface;
  ctx.fillRect(0, 0, width, height);
  ctx.textBaseline = 'alphabetic';

  let y = pad;
  if (header.title) {
    ctx.font = font(500, 12);
    ctx.fillStyle = theme.ink.secondary;
    ctx.fillText(header.title, pad, y + 12);
    y += 20;
  }
  ctx.font = font(500, 15);
  ctx.fillStyle = theme.ink.primary;
  for (const line of takeaway) {
    ctx.fillText(line, pad, y + 15);
    y += 21;
  }
  if (headerHeight) y += 10;

  const origin = { x: pad - box.left, y: y - box.top };
  for (const layer of root.querySelectorAll<HTMLElement | SVGSVGElement>('[data-chart-layer], [data-chart-legend]')) {
    if (layer.getAttribute('data-chart-export') === 'skip') continue;
    const r = layer.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) continue;
    if (layer instanceof HTMLCanvasElement) {
      ctx.drawImage(layer, r.left + origin.x, r.top + origin.y, r.width, r.height);
    } else if (layer instanceof SVGSVGElement) {
      ctx.drawImage(await svgImage(layer, r.width, r.height), r.left + origin.x, r.top + origin.y, r.width, r.height);
    } else {
      // An HTML legend: redraw each item's swatch and text where it sits.
      ctx.font = font(400, 12);
      for (const item of layer.querySelectorAll<HTMLElement>('li')) {
        const swatch = item.firstElementChild as HTMLElement | null;
        const ir = item.getBoundingClientRect();
        if (swatch) {
          const sr = swatch.getBoundingClientRect();
          const style = getComputedStyle(swatch);
          ctx.fillStyle = style.backgroundColor !== 'rgba(0, 0, 0, 0)' ? style.backgroundColor : style.borderTopColor;
          ctx.fillRect(sr.left + origin.x, sr.top + origin.y, Math.max(sr.width, 2), Math.max(sr.height, 2));
          ctx.fillStyle = theme.ink.secondary;
          ctx.fillText((item.textContent ?? '').trim(), sr.right + origin.x + 6, ir.top + origin.y + ir.height / 2 + 4);
        }
      }
    }
  }

  y += Math.round(box.height) + 10;
  ctx.font = font(400, 11);
  ctx.fillStyle = theme.label;
  for (const line of caption) {
    ctx.fillText(line, pad, y + 11);
    y += 15;
  }

  return new Promise((resolve, reject) => canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error('The image could not be encoded.'))), 'image/png'));
}

/** Copies a PNG to the clipboard; where that is not allowed, downloads it. Returns what happened. */
export async function copyOrDownloadPng(make: () => Promise<Blob>, filename: string): Promise<'copied' | 'downloaded'> {
  try {
    if (!('ClipboardItem' in window) || !navigator.clipboard?.write) throw new Error('no clipboard');
    // Safari needs the promise handed over inside the user gesture.
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': make() })]);
    return 'copied';
  } catch {
    downloadBlob(await make(), filename);
    return 'downloaded';
  }
}
