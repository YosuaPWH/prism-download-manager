import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Bootstrap, CoreEvent, DownloadItem, Settings } from './types';

const bridge = vi.hoisted(() => ({
  bootstrap: vi.fn<() => Promise<Bootstrap>>(),
  nativeMaterial: vi.fn(async () => false),
  off: vi.fn(),
  handler: undefined as undefined | ((event: CoreEvent) => void),
  listen: vi.fn(),
}));
vi.mock('./api', () => ({
  api: { bootstrap: bridge.bootstrap, nativeMaterial: bridge.nativeMaterial },
  onCoreEvent: bridge.listen,
}));
const { useApp } = await import('./store');

const download = (status: 'downloading' | 'completed') => ({ id: 'a', status } as DownloadItem);
const snapshot = (): Bootstrap => ({
  version: 'test', dataDir: '/test', items: [download('downloading')],
  categories: [], queues: [], grabberProjects: [], captures: [],
  settings: { appearance: { theme: 'dark', accent: 'prism', density: 'compact', motion: 'reduced' } } as Settings,
});
afterEach(() => vi.unstubAllGlobals());
beforeEach(() => {
  vi.clearAllMocks();
  bridge.handler = undefined;
  bridge.listen.mockImplementation(async (handler: (event: CoreEvent) => void) => {
    bridge.handler = handler;
    return bridge.off;
  });
  vi.stubGlobal('document', { documentElement: { dataset: {}, classList: { toggle: vi.fn() } } });
  vi.stubGlobal('window', {});
  useApp.setState({ ready: false, items: {}, progress: {}, selection: [] });
});

describe('bootstrap/event ordering', () => {
  it('subscribes before requesting the snapshot and replays events received during bootstrap', async () => {
    let resolve!: (boot: Bootstrap) => void;
    bridge.bootstrap.mockImplementation(() => new Promise((r) => { resolve = r; }));
    const pending = useApp.getState().init();
    await vi.waitFor(() => expect(bridge.bootstrap).toHaveBeenCalledTimes(1));
    expect(bridge.handler, 'event listener must be ready before the snapshot').toBeDefined();
    bridge.handler!({ type: 'itemUpdated', payload: download('completed') });
    resolve(snapshot());
    await pending;
    expect(useApp.getState().items.a.status).toBe('completed');
    expect(useApp.getState().ready).toBe(true);
  });

  it('shares concurrent initialization rather than subscribing twice', async () => {
    bridge.bootstrap.mockResolvedValue(snapshot());
    await Promise.all([useApp.getState().init(), useApp.getState().init()]);
    expect(bridge.bootstrap).toHaveBeenCalledTimes(1);
    expect(bridge.listen).toHaveBeenCalledTimes(1);
  });

  it('unsubscribes after a failed bootstrap so retrying cannot duplicate event handling', async () => {
    bridge.bootstrap.mockRejectedValueOnce(new Error('bootstrap failed'));
    await expect(useApp.getState().init()).rejects.toThrow('bootstrap failed');
    expect(bridge.off).toHaveBeenCalledTimes(1);
    bridge.bootstrap.mockResolvedValue(snapshot());
    await useApp.getState().init();
    expect(useApp.getState().ready).toBe(true);
    expect(bridge.listen).toHaveBeenCalledTimes(2);
  });
});
