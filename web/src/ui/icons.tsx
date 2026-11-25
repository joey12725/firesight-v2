// A hand-picked subset of Lucide (https://lucide.dev, ISC licence), inlined so the
// bundle carries only what the app draws. Add an icon by pasting its path data here.
import type { ReactNode, SVGProps } from 'react';

const c = (cx: number, cy: number, r: number) => <circle cx={cx} cy={cy} r={r} />;
const p = (...d: string[]) => d.map((x) => <path key={x} d={x} />);
const r = (x: number, y: number, w: number, h: number, rx = 2) => <rect x={x} y={y} width={w} height={h} rx={rx} />;

const ICONS = {
  activity: p('M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2'),
  alertCircle: [c(12, 12, 10), ...p('M12 8v4', 'M12 16h.01')],
  alertTriangle: p('m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3', 'M12 9v4', 'M12 17h.01'),
  arrowRight: p('M5 12h14', 'm12 5 7 7-7 7'),
  bookOpen: p('M12 7v14', 'M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z'),
  briefcase: [...p('M16 20V4a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16'), r(2, 6, 20, 14)],
  calendar: [...p('M8 2v4', 'M16 2v4', 'M3 10h18'), r(3, 4, 18, 18)],
  chart: p('M3 3v16a2 2 0 0 0 2 2h16', 'M18 17V9', 'M13 17V5', 'M8 17v-3'),
  check: p('M20 6 9 17l-5-5'),
  checkCircle: [c(12, 12, 10), ...p('m9 12 2 2 4-4')],
  chevronDown: p('m6 9 6 6 6-6'),
  chevronLeft: p('m15 18-6-6 6-6'),
  chevronRight: p('m9 18 6-6-6-6'),
  chevronUp: p('m18 15-6-6-6 6'),
  cloud: p('M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z'),
  cloudOff: p('m2 2 20 20', 'M5.782 5.782A7 7 0 0 0 9 19h8.5a4.5 4.5 0 0 0 1.307-.193', 'M21.532 16.5A4.5 4.5 0 0 0 17.5 10h-1.79A7.008 7.008 0 0 0 10 5.07'),
  copy: [r(8, 8, 14, 14), ...p('M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2')],
  creditCard: [r(2, 5, 20, 14), ...p('M2 10h20')],
  download: p('M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4', 'm7 10 5 5 5-5', 'M12 15V3'),
  eye: [...p('M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0'), c(12, 12, 3)],
  fileText: p('M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z', 'M14 2v4a2 2 0 0 0 2 2h4', 'M10 9H8', 'M16 13H8', 'M16 17H8'),
  flame: p('M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5z'),
  folder: p('M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z'),
  gitBranch: [...p('M6 3v12', 'M18 9a9 9 0 0 1-9 9'), c(18, 6, 3), c(6, 18, 3)],
  help: [c(12, 12, 10), ...p('M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3', 'M12 17h.01')],
  home: p('M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8', 'M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z'),
  info: [c(12, 12, 10), ...p('M12 16v-4', 'M12 8h.01')],
  key: [...p('m15.5 7.5 2.3 2.3a1 1 0 0 0 1.4 0l2.1-2.1a1 1 0 0 0 0-1.4L19 4', 'm21 2-9.6 9.6'), c(7.5, 15.5, 5.5)],
  landmark: p('M3 22h18', 'M6 18v-7', 'M10 18v-7', 'M14 18v-7', 'M18 18v-7', 'm12 2 8 5H4z'),
  layers: p('m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z', 'm22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65', 'm22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65'),
  link: p('M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71', 'M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71'),
  lock: [r(3, 11, 18, 11), ...p('M7 11V7a5 5 0 0 1 10 0v4')],
  logOut: p('M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4', 'm16 17 5-5-5-5', 'M21 12H9'),
  mapPin: [...p('M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0'), c(12, 10, 3)],
  menu: p('M4 12h16', 'M4 6h16', 'M4 18h16'),
  message: p('M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z'),
  minus: p('M5 12h14'),
  monitor: [r(2, 3, 20, 14), ...p('M8 21h8', 'M12 17v4')],
  moon: p('M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z'),
  more: [c(12, 12, 1), c(19, 12, 1), c(5, 12, 1)],
  panelLeft: [r(3, 3, 18, 18), ...p('M9 3v18')],
  panelRight: [r(3, 3, 18, 18), ...p('M15 3v18')],
  pencil: p('M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z', 'm15 5 4 4'),
  plus: p('M5 12h14', 'M12 5v14'),
  redo: p('m15 14 5-5-5-5', 'M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13'),
  rocket: p(
    'M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z',
    'm12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z',
    'M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0',
    'M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5',
  ),
  rotate: p('M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8', 'M21 3v5h-5'),
  search: [c(11, 11, 8), ...p('m21 21-4.3-4.3')],
  settings: [...p('M20 7h-9', 'M14 17H5'), c(17, 17, 3), c(7, 7, 3)],
  share: [c(18, 5, 3), c(6, 12, 3), c(18, 19, 3), ...p('m8.59 13.51 6.83 3.98', 'm15.41 6.51-6.82 3.98')],
  table: [r(3, 3, 18, 18), ...p('M3 9h18', 'M3 15h18', 'M12 3v18')],
  image: [r(3, 3, 18, 18), c(9, 9, 2), ...p('m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21')],
  sliders: p('M21 4h-7', 'M10 4H3', 'M21 12h-9', 'M8 12H3', 'M21 20h-5', 'M12 20H3', 'M14 2v4', 'M8 10v4', 'M16 18v4'),
  sparkles: p('M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z', 'M20 3v4', 'M22 5h-4', 'M4 17v2', 'M5 18H3'),
  sun: [c(12, 12, 4), ...p('M12 2v2', 'M12 20v2', 'm4.93 4.93 1.41 1.41', 'm17.66 17.66 1.41 1.41', 'M2 12h2', 'M20 12h2', 'm6.34 17.66-1.41 1.41', 'm19.07 4.93-1.41 1.41')],
  sunrise: p('M12 2v8', 'm4.93 10.93 1.41 1.41', 'M2 18h2', 'M20 18h2', 'm19.07 10.93-1.41 1.41', 'M22 22H2', 'm8 6 4-4 4 4', 'M16 18a4 4 0 0 0-8 0'),
  trash: p('M3 6h18', 'M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6', 'M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2', 'M10 11v6', 'M14 11v6'),
  trendingUp: p('M22 7 13.5 15.5l-5-5L2 17', 'M16 7h6v6'),
  undo: p('M9 14 4 9l5-5', 'M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11'),
  upload: p('M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4', 'm17 8-5-5-5 5', 'M12 3v12'),
  user: [...p('M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2'), c(12, 7, 4)],
  users: [...p('M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2', 'M22 21v-2a4 4 0 0 0-3-3.87', 'M16 3.13a4 4 0 0 1 0 7.75'), c(9, 7, 4)],
  x: p('M18 6 6 18', 'm6 6 12 12'),
} satisfies Record<string, ReactNode[]>;

export type IconName = keyof typeof ICONS;

export interface IconProps extends Omit<SVGProps<SVGSVGElement>, 'name'> {
  name: IconName;
  /** Pixel size; icons sit on a 24 grid and are drawn at 16 by default. */
  size?: number;
}

/** Decorative by default. Give it a `aria-label` when it stands alone as content. */
export function Icon({ name, size = 16, strokeWidth = 1.75, ...rest }: IconProps) {
  const labelled = rest['aria-label'] !== undefined;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden={labelled ? undefined : true}
      role={labelled ? 'img' : undefined}
      focusable="false"
      {...rest}
      className={`shrink-0 ${rest.className ?? ''}`}
    >
      {ICONS[name].map((child, i) => (
        <g key={i}>{child}</g>
      ))}
    </svg>
  );
}
