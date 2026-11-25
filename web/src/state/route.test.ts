// Which page an address names, and where the login page may send someone afterwards.
import { describe, expect, it } from 'vitest';
import { loginPath, parsePage, planPath, safeNext } from './route';

const ID = 'pl_0123456789ABCDEFGHIJkl';
const ORIGIN = 'https://firesight.example';

describe('parsePage', () => {
  it('reserves the account pages', () => {
    expect(parsePage('/login')).toEqual({ kind: 'login' });
    expect(parsePage('/login/')).toEqual({ kind: 'login' });
    expect(parsePage('/consent', '?client_id=x&sig=y')).toEqual({ kind: 'consent' });
    expect(parsePage('/account')).toEqual({ kind: 'account' });
  });

  it('reads a plan link, with a view only when it names one', () => {
    expect(parsePage(`/p/${ID}`)).toEqual({ kind: 'plan', id: ID, view: null });
    expect(parsePage(`/p/${ID}`, '?view=cash-flow')).toEqual({ kind: 'plan', id: ID, view: 'cash-flow' });
    expect(parsePage(`/p/${ID}`, '?view=nonsense')).toEqual({ kind: 'plan', id: ID, view: null });
    expect(parsePage(`/p/${ID}`, '?view=<script>')).toEqual({ kind: 'plan', id: ID, view: null });
  });

  it('treats everything else as the app, including near misses', () => {
    for (const path of ['/', '/overview', '/cash-flow', '/life-stories', '/not-a-view', '/login/extra', '/account/keys', '/p', '/p/', `/p/${ID}/more`, '/p/pl_short', `/p/${ID}x`, '/p/h_abc123', `/P/${ID}`, '/Login']) {
      expect(parsePage(path), path).toEqual({ kind: 'app' });
    }
  });
});

describe('safeNext', () => {
  it('keeps a path on this site, with its query', () => {
    expect(safeNext('/account', ORIGIN)).toBe('/account');
    expect(safeNext(`/p/${ID}?view=outcomes`, ORIGIN)).toBe(`/p/${ID}?view=outcomes`);
    expect(safeNext('/cash-flow', ORIGIN)).toBe('/cash-flow');
  });

  it('falls back to / for anything that could leave the site', () => {
    for (const next of [null, undefined, '', 'account', 'https://evil.example/', '//evil.example', '/\\evil.example', '/\\/evil.example', '\\\\evil.example', 'javascript:alert(1)', '/ok\nSet-Cookie: x', '/a\\b', ' /account', '/\t/evil.example']) {
      expect(safeNext(next, ORIGIN), JSON.stringify(next)).toBe('/');
    }
  });

  it('does not loop back to the login or consent page', () => {
    expect(safeNext('/login', ORIGIN)).toBe('/');
    expect(safeNext('/login?next=/login', ORIGIN)).toBe('/');
    expect(safeNext('/consent?client_id=x', ORIGIN)).toBe('/');
  });
});

describe('loginPath and planPath', () => {
  it('round-trip through parsePage and safeNext', () => {
    expect(planPath(ID)).toBe(`/p/${ID}`);
    expect(planPath(ID, 'overview')).toBe(`/p/${ID}`);
    expect(planPath(ID, 'cash-flow')).toBe(`/p/${ID}?view=cash-flow`);
    const login = new URL(loginPath(planPath(ID, 'cash-flow')), ORIGIN);
    expect(parsePage(login.pathname, login.search)).toEqual({ kind: 'login' });
    const back = safeNext(login.searchParams.get('next'), ORIGIN);
    const url = new URL(back, ORIGIN);
    expect(parsePage(url.pathname, url.search)).toEqual({ kind: 'plan', id: ID, view: 'cash-flow' });
    expect(loginPath()).toBe('/login');
    expect(loginPath('/')).toBe('/login');
  });
});
