import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  getAll: vi.fn(async (_details: chrome.cookies.GetAllDetails) => [] as Array<{ name: string; value: string }>),
  getAllCookieStores: vi.fn(async () => [] as Array<{ id: string; incognito?: boolean }>),
}));
vi.mock('../src/shared/api', () => ({
  IS_FIREFOX: false,
  api: { runtime: { getManifest: () => ({ version: 'test' }) }, cookies: mocks },
}));
vi.mock('../src/background/state', () => ({ getConnection: () => ({ state: 'connected' }) }));
const { cookieCache, cookieHeaderFor } = await import('../src/background/context');

beforeEach(() => {
  mocks.getAll.mockReset();
  mocks.getAllCookieStores.mockReset();
  mocks.getAllCookieStores.mockResolvedValue([]);
});
afterEach(() => vi.restoreAllMocks());

describe('cookie lookup isolation', () => {
  it('does not reuse cookies between different paths in the same folder', async () => {
    mocks.getAll.mockImplementation(async ({ url }) => new URL(url!).pathname === '/files/private'
      ? [{ name: 'private', value: 'path-scoped' }] : []);
    const cookies = cookieCache(false);
    expect(await cookies('https://example.test/files/private')).toBe('private=path-scoped');
    expect(await cookies('https://example.test/files/public')).toBe('');
  });

  it('shares a lookup only when cookie origin and path match', async () => {
    mocks.getAll.mockResolvedValue([{ name: 'sid', value: 'one' }]);
    const cookies = cookieCache(false);
    await cookies('https://example.test/files/file?part=1');
    await cookies('https://example.test/files/file?part=2');
    expect(mocks.getAll).toHaveBeenCalledTimes(1);
  });

  it('never falls back to the regular cookie store for an unresolved private store', async () => {
    mocks.getAll.mockResolvedValue([{ name: 'regular', value: 'private-account' }]);
    expect(await cookieHeaderFor('https://example.test/file', true, 'private=sent')).toBe('private=sent');
    expect(mocks.getAll).not.toHaveBeenCalled();
  });

  it('uses the resolved private store when it is available', async () => {
    mocks.getAllCookieStores.mockResolvedValue([{ id: 'private-store', incognito: true }]);
    mocks.getAll.mockResolvedValue([{ name: 'sid', value: 'private-session' }]);
    expect(await cookieHeaderFor('https://example.test/file', true)).toBe('sid=private-session');
    expect(mocks.getAll).toHaveBeenCalledWith({ url: 'https://example.test/file', storeId: 'private-store' });
  });
});
