import { beforeEach, describe, expect, it } from "vitest";
import { handleEvent, useApp } from "./store";
import type { DownloadItem, Progress, Status } from "./types";

const item = (status: Status): DownloadItem => ({
  id: "a",
  kind: "http",
  url: "https://example.com/file.zip",
  fileName: "file.zip",
  saveDir: "/downloads",
  size: 1000,
  downloaded: 1000,
  status,
  category: "compressed",
  description: "",
  addedAt: 0,
  connections: 8,
  speedLimit: 0,
  trashed: false,
  position: 0,
  mirrors: [],
});
const progress = (downloaded: number): Progress => ({ id: "a", status: "downloading", downloaded, size: 1000, speed: 5000, connections: 8 });

describe("live progress (#41)", () => {
  beforeEach(() => useApp.setState({ items: { a: item("downloading") }, progress: {} }));

  it("progress that arrives after the download finished doesn't bring it back", () => {
    handleEvent({ type: "progress", payload: [progress(900)] });
    expect(useApp.getState().progress.a?.downloaded).toBe(900);
    handleEvent({ type: "itemUpdated", payload: item("completed") });
    expect(useApp.getState().progress.a).toBeUndefined();
    // A late tick: the row must stay completed.
    handleEvent({ type: "progress", payload: [progress(1000)] });
    expect(useApp.getState().progress.a).toBeUndefined();
    expect(useApp.getState().items.a.status).toBe("completed");
  });

  it("a download started again shows its progress", () => {
    handleEvent({ type: "itemUpdated", payload: item("completed") });
    handleEvent({ type: "itemUpdated", payload: item("connecting") });
    handleEvent({ type: "progress", payload: [progress(10)] });
    expect(useApp.getState().progress.a?.downloaded).toBe(10);
  });

  it("the whole list sent again drops the progress of downloads that stopped", () => {
    handleEvent({ type: "progress", payload: [progress(500)] });
    handleEvent({ type: "itemsAdded", payload: [item("completed")] });
    expect(useApp.getState().progress.a).toBeUndefined();
    expect(useApp.getState().items.a.status).toBe("completed");
  });

  it("an authoritative snapshot removes deleted rows, stale progress and selection", () => {
    useApp.setState({ selection: ['a'], anchor: 'a', progress: { a: progress(500) } });
    handleEvent({ type: 'itemsSnapshot', payload: [] });
    expect(useApp.getState().items).toEqual({});
    expect(useApp.getState().progress).toEqual({});
    expect(useApp.getState().selection).toEqual([]);
    expect(useApp.getState().anchor).toBeNull();
  });

  it("a snapshot keeps live progress only for downloads still active in that snapshot", () => {
    useApp.setState({ progress: { a: progress(500), orphan: { ...progress(2), id: 'orphan' } } });
    handleEvent({ type: 'itemsSnapshot', payload: [item('downloading')] });
    expect(Object.keys(useApp.getState().progress)).toEqual(['a']);
    handleEvent({ type: 'itemsSnapshot', payload: [item('paused')] });
    expect(useApp.getState().progress).toEqual({});
  });

  it('ignores late progress for a download already removed from the list', () => {
    handleEvent({ type: 'itemsRemoved', payload: ['a'] });
    handleEvent({ type: 'progress', payload: [progress(999)] });
    expect(useApp.getState().progress).toEqual({});
  });
});
