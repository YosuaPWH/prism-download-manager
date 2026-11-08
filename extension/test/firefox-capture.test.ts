import { afterEach, describe, expect, it, vi } from 'vitest';
import { defaultState } from '../src/shared/settings';

const bridge = vi.hoisted(() => ({
  headers: undefined as undefined | ((details: unknown) => Promise<unknown>),
  created: undefined as undefined | ((item: unknown) => void),
  native: vi.fn(async () => ({ ok: true, accepted: true })),
  cancel: vi.fn(async () => {}),
  erase: vi.fn(async () => []),
}));
vi.mock('../src/shared/api', () => ({ api: {
  runtime: { id: 'test' },
  webRequest: { onHeadersReceived: { addListener: (fn: typeof bridge.headers) => { bridge.headers = fn; } } },
  downloads: {
    onCreated: { addListener: (fn: typeof bridge.created) => { bridge.created = fn; } },
    cancel: bridge.cancel, erase: bridge.erase,
  },
} }));
vi.mock('../src/background/native', () => ({ native: { request: bridge.native } }));
vi.mock('../src/background/state', () => ({ getState: async () => defaultState() }));
vi.mock('../src/background/badge', () => ({ showUnreachable: vi.fn() }));
vi.mock('../src/background/requests', () => ({ requestByUrl: () => undefined }));
vi.mock('../src/background/context', () => ({
  actionTimeout: () => 1000, cookieHeaderFor: async () => '',
  tabInfo: async () => undefined, userAgent: () => 'test',
}));
const { initFirefoxCapture } = await import('../src/background/capture');
afterEach(() => vi.useRealTimers());

describe('Firefox handoff deduplication', () => {
  it('suppresses the immediate duplicate but captures the same URL again after expiry', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(0);
    initFirefoxCapture();
    const url = 'https://example.test/repeated.zip';
    await bridge.headers!({
      statusCode: 200, method: 'GET', url, incognito: false,
      responseHeaders: [{ name: 'content-disposition', value: 'attachment; filename="repeated.zip"' }],
    });
    expect(bridge.native).toHaveBeenCalledTimes(1);
    const item = { id: 1, state: 'in_progress', url, filename: 'repeated.zip', incognito: false, totalBytes: 1024 };
    bridge.created!(item);
    await Promise.resolve();
    expect(bridge.native).toHaveBeenCalledTimes(1);
    vi.setSystemTime(31_000);
    bridge.created!({ ...item, id: 2 });
    await vi.waitFor(() => expect(bridge.native).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(bridge.cancel).toHaveBeenCalledWith(2));
  });
});
