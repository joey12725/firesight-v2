import { useId, useRef, useState } from 'react';
import { CONTROL, Icon, cx } from '@/ui';
import { loadCities, searchCities, type City } from './cities';

interface CitySearchProps {
  value: string | null;
  /** Called with a city from the table, or with free text when the user types their own. */
  onPick: (city: City | null, text: string) => void;
  id?: string;
  invalid?: boolean;
  placeholder?: string;
}

/** A combobox over the cost-of-living table. The table is fetched when the field is first focused. */
export function CitySearch({ value, onPick, id, invalid, placeholder = 'Search 4,400 cities' }: CitySearchProps) {
  const [text, setText] = useState(value ?? '');
  const [cities, setCities] = useState<City[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const listId = useId();
  const wrapper = useRef<HTMLDivElement>(null);

  // Follow outside changes to the value (undo, another editor) without an effect.
  const [seen, setSeen] = useState(value);
  if (seen !== value) {
    setSeen(value);
    setText(value ?? '');
  }

  const ensureLoaded = () => {
    if (cities || failed) return;
    loadCities().then(setCities, () => setFailed(true));
  };

  const matches = cities && open ? searchCities(cities, text) : [];
  const choose = (c: City) => {
    setText(c.city);
    setOpen(false);
    onPick(c, c.city);
  };

  return (
    <div
      ref={wrapper}
      className="relative"
      onBlur={(e) => {
        if (!wrapper.current?.contains(e.relatedTarget)) {
          setOpen(false);
          if (text !== (value ?? '')) onPick(null, text.trim());
        }
      }}
    >
      <Icon name="search" size={13} className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-ink-3" />
      <input
        id={id}
        type="text"
        role="combobox"
        aria-expanded={open && matches.length > 0}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={open && matches[active] ? `${listId}-${active}` : undefined}
        aria-invalid={invalid || undefined}
        autoComplete="off"
        spellCheck={false}
        placeholder={placeholder}
        value={text}
        onFocus={ensureLoaded}
        onChange={(e) => {
          setText(e.target.value);
          setOpen(true);
          setActive(0);
        }}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown') {
            e.preventDefault();
            setOpen(true);
            setActive((a) => Math.min(a + 1, Math.max(matches.length - 1, 0)));
          } else if (e.key === 'ArrowUp') {
            e.preventDefault();
            setActive((a) => Math.max(a - 1, 0));
          } else if (e.key === 'Enter' && open && matches[active]) {
            e.preventDefault();
            choose(matches[active]);
          } else if (e.key === 'Escape' && open) {
            e.stopPropagation();
            setOpen(false);
          }
        }}
        className={cx(CONTROL, invalid ? 'border-bad' : 'border-line', 'w-full pl-7 pr-2')}
      />
      {open && text.trim().length >= 2 && (
        <ul id={listId} role="listbox" className="absolute inset-x-0 top-full z-30 mt-1 max-h-64 overflow-auto rounded-md border border-line bg-surface-2 py-1 shadow-overlay">
          {matches.map((c, i) => (
            <li
              key={c.search}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === active}
              onPointerDown={(e) => {
                e.preventDefault();
                choose(c);
              }}
              onPointerEnter={() => setActive(i)}
              className={cx('flex cursor-pointer items-baseline justify-between gap-3 px-2.5 py-1.5 text-sm', i === active && 'bg-surface-3')}
            >
              <span className="truncate">
                {c.city} <span className="text-xs text-ink-3">{c.country}</span>
              </span>
              <span className="num shrink-0 text-xs text-ink-3" title="Cost-of-living index for one adult; 100 is the US median city">
                {c.overallSingle.toFixed(0)}
              </span>
            </li>
          ))}
          {matches.length === 0 && (
            <li className="px-2.5 py-1.5 text-xs text-ink-3">
              {failed ? 'The city table could not be loaded. You can still type a name.' : cities ? 'No city in the table matches. The name will be kept as typed.' : 'Loading cities…'}
            </li>
          )}
        </ul>
      )}
    </div>
  );
}
