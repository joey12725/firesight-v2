// The cost-of-living table (data/col/cities.csv, copied to /data at build time),
// loaded on first use. See data/col/README.md for how the indices are built.
import type { Category } from '@/engine/types';

export interface City {
  city: string;
  country: string;
  /** ISO 3166 two-letter code when the country name could be matched, else ''. */
  countryCode: string;
  /** 1 when the source had enough contributors for this city. */
  quality: number;
  overallSingle: number;
  overallFamily: number;
  rent: number;
  homePrice: number;
  groceries: number;
  dining: number;
  transport: number;
  utilities: number;
  childcare: number;
  goodsServices: number;
  search: string;
}

let loading: Promise<City[]> | null = null;

let countryList: { value: string; label: string }[] | null = null;

/** Every country by its English name, the United States first: the options of a country field. */
export function countryOptions(): { value: string; label: string }[] {
  if (countryList) return countryList;
  const names = new Intl.DisplayNames(['en'], { type: 'region' });
  const codes = new Set(countryCodes().values());
  const rest = [...codes]
    .filter((code) => code !== 'US')
    .map((code) => ({ value: code, label: names.of(code) ?? code }))
    .sort((a, b) => a.label.localeCompare(b.label));
  countryList = [{ value: 'US', label: 'United States' }, ...rest];
  return countryList;
}

function countryCodes(): Map<string, string> {
  const names = new Intl.DisplayNames(['en'], { type: 'region' });
  const map = new Map<string, string>();
  const A = 65;
  for (let i = 0; i < 26; i++) {
    for (let j = 0; j < 26; j++) {
      const code = String.fromCharCode(A + i, A + j);
      let name: string | undefined;
      try {
        name = names.of(code);
      } catch {
        continue;
      }
      if (name && name !== code) map.set(name.toLowerCase(), code);
    }
  }
  // Names the source spells differently from CLDR.
  const aliases: Record<string, string> = {
    'czech republic': 'CZ',
    'hong kong': 'HK',
    macau: 'MO',
    'bosnia and herzegovina': 'BA',
    'ivory coast': 'CI',
    myanmar: 'MM',
    turkey: 'TR',
    'cape verde': 'CV',
    'palestinian territory': 'PS',
    'trinidad and tobago': 'TT',
    'antigua and barbuda': 'AG',
    'saint kitts and nevis': 'KN',
    'saint vincent and the grenadines': 'VC',
    'sao tome and principe': 'ST',
    'east timor': 'TL',
    swaziland: 'SZ',
    'us virgin islands': 'VI',
    'democratic republic of the congo': 'CD',
    'republic of the congo': 'CG',
    'kosovo (disputed territory)': 'XK',
  };
  for (const [name, code] of Object.entries(aliases)) if (!map.has(name)) map.set(name, code);
  return map;
}

function parse(text: string): City[] {
  const codes = countryCodes();
  const lines = text.trim().split('\n');
  const out: City[] = [];
  // The file has no quoted fields; a plain split is exact.
  for (const line of lines.slice(1)) {
    const f = line.split(',');
    if (f.length < 13) continue;
    const n = (i: number) => Number(f[i]);
    const city = f[0] as string;
    const country = f[1] as string;
    out.push({
      city,
      country,
      countryCode: codes.get(country.toLowerCase()) ?? '',
      quality: n(2),
      overallSingle: n(3),
      overallFamily: n(4),
      rent: n(5),
      homePrice: n(6),
      groceries: n(7),
      dining: n(8),
      transport: n(9),
      utilities: n(10),
      childcare: n(11),
      goodsServices: n(12),
      search: `${city}, ${country}`.toLowerCase(),
    });
  }
  return out;
}

export function loadCities(): Promise<City[]> {
  loading ??= fetch(`${import.meta.env.BASE_URL}data/cities.csv`)
    .then((r) => {
      if (!r.ok) throw new Error(`cities.csv: ${r.status}`);
      return r.text();
    })
    .then(parse)
    .catch((error: unknown) => {
      loading = null;
      throw error;
    });
  return loading;
}

/** Best matches for a query: prefix matches first, well-sampled cities before thin ones. */
export function searchCities(cities: City[], query: string, limit = 8): City[] {
  const q = query.trim().toLowerCase();
  if (q.length < 2) return [];
  const scored: { c: City; score: number }[] = [];
  for (const c of cities) {
    const at = c.search.indexOf(q);
    if (at < 0) continue;
    scored.push({ c, score: (at === 0 ? 0 : 10) + (c.quality ? 0 : 3) + (c.country === 'United States' ? 0 : 1) + c.city.length / 100 });
  }
  return scored
    .sort((a, b) => a.score - b.score)
    .slice(0, limit)
    .map((s) => s.c);
}

export function findCity(cities: City[], city: string | null, countryCode: string): City | undefined {
  if (!city) return undefined;
  const name = city.toLowerCase();
  const matches = cities.filter((c) => c.city.toLowerCase() === name);
  return matches.find((c) => c.countryCode === countryCode) ?? (matches.length === 1 ? matches[0] : undefined);
}

/**
 * Per-category spending multipliers for a move, as destination index / origin index.
 * Follows data/col/budget_mapping.json: fixed contracts and nationally priced lines
 * (healthcare, insurance, giving, education, travel) are left unchanged.
 */
export function relocationFactors(from: City, to: City, family: boolean): Partial<Record<Category, number>> {
  const ratio = (a: number, b: number) => Math.round((b / a) * 1000) / 1000;
  return {
    rent: ratio(from.rent, to.rent),
    utilities: ratio(from.utilities, to.utilities),
    groceries: ratio(from.groceries, to.groceries),
    dining: ratio(from.dining, to.dining),
    transport: ratio(from.transport, to.transport),
    childcare: ratio(from.childcare, to.childcare),
    entertainment: ratio(from.goodsServices, to.goodsServices),
    personal: ratio(from.goodsServices, to.goodsServices),
    other: family ? ratio(from.overallFamily, to.overallFamily) : ratio(from.overallSingle, to.overallSingle),
  };
}
