// Copies the cost-of-living table into public/data so the editor can lazy-load it.
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const web = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dest = resolve(web, 'public/data');
mkdirSync(dest, { recursive: true });
copyFileSync(resolve(web, '../data/col/cities.csv'), resolve(dest, 'cities.csv'));
console.log('data: copied cities.csv');
