import type { EquityPreview } from '@/engine/equity-types';
import type { PrivateCompany } from '@/engine/types';
import { Tag, moneyCompact } from '@/ui';
import { QuantileDots } from '@/ui/charts/ext/outcomes-QuantileDots';
import { isOption } from './model';
import { intervalText, previewCaption, previewNotes, previewSentences } from './text';
import { canShowPayoffs } from './usePreview';

const range = (lo: number, hi: number) => `${moneyCompact(lo)} to ${moneyCompact(hi)}`;

/**
 * What the holdings could pay, as 100 equally likely futures: the first thing in an
 * open company. It leads with how often the answer is nothing, because the paper value
 * is the most flattering number available.
 */
export function PreviewStrip({ company, preview }: { company: PrivateCompany; preview: EquityPreview | null }) {
  const excluded = !company.included && (
    <Tag tone="warn" icon="alertTriangle">
      Not counted in the plan
    </Tag>
  );
  if (!canShowPayoffs(company)) {
    return (
      <div className="space-y-2">
        <p className="text-xs leading-relaxed text-ink-3">Enter the latest price per share and what you hold to see what it could pay.</p>
        {excluded}
      </div>
    );
  }
  if (!preview) return excluded || null;
  const hasOptions = company.holdings.some(isOption);
  const [p10, , p50, , p90] = preview.quantiles;
  return (
    <div data-testid="equity-preview" className="space-y-2">
      {excluded}
      <div className="space-y-1 text-sm leading-relaxed text-ink-1">
        {[...previewSentences(preview, hasOptions, company), ...previewNotes(preview, company)].map((sentence) => (
          <p key={sentence}>{sentence}</p>
        ))}
      </div>
      <div className="rounded-md bg-surface-2 px-1 pt-2">
        <QuantileDots
          dots={preview.dots}
          unit="futures"
          scale="log"
          groupOf={(v) => (v === null || v <= 0 ? 'nothing' : null)}
          groups={[{ key: 'nothing', label: 'Nothing', side: 'left', hollow: true, note: 'The company fails, is sold for too little to reach common stock, or the options end up under water or lapse.' }]}
          format={moneyCompact}
          formatRange={range}
          axis={{ kind: 'value', format: 'money', title: 'Paid out, before tax' }}
          noun={`What ${company.name || 'this company'} could pay`}
          interval={{ p10: p10 > 0 ? p10 : null, p50: p50 > 0 ? p50 : null, p90: p90 > 0 ? p90 : null }}
          intervalText={intervalText(preview)}
          height={140 + 38 + 30}
        />
      </div>
      <p className="text-xs leading-relaxed text-ink-3">{previewCaption(preview, company.outcomes.length > 0, hasOptions, company)}</p>
    </div>
  );
}
