# Code audit: download integrity, credentials, and lifecycle races

This review starts from fork commit `eab7069`, after the initial enhancements and English
README were merged. It examines HTTP authentication and redirects, final-file publication,
pause/cancel handling, browser cookie context, extension handoff tracking, and frontend
startup/event recovery.

## Findings and fixes

| ID | Priority | Finding | Result |
| --- | --- | --- | --- |
| A1 | High | Subsequent direct range requests could send the origin's cookies and generated HTTP credentials to a redirect target. Mirror validation also reused origin credentials. | Bind credentials to the initial request origin, strip sensitive headers on other origins, and reject foreign authentication challenges. |
| A2 | High | Checksum verification calculated a digest, then renamed the partial file before rejecting a mismatch. With Overwrite, an invalid download replaced the previous file. | Compare the checksum before publication. Preserve the existing destination and keep rejected bytes at the partial-file path reported by the error. |
| A3 | Medium | A supervisor's captured Pause outcome could override a later Cancel. Cancellation during finalization could also leave hashing running or publish the file. | Recheck the latest stop reason before and after draining the writer, poll cancellation between checksum chunks, and check again before final publication. |
| A4 | Medium | Content-Range parsing accepted an end outside the total or an end whose exclusive bound overflowed. Repeated malformed 206 responses could keep the probe looping. | Reject impossible ranges and allow only one fallback to a plain request before failing. |
| A5 | Medium | Batch cookie caching used a containing folder as its key, although cookies can have more specific paths. | Cache by origin and exact path; query-string changes can still share a lookup. |
| A6 | High | An unresolved incognito cookie store fell back to an unqualified lookup, which selects the regular store. | Preserve only cookies already captured from the private request when its store cannot be resolved. |
| A7 | Medium | Frontend bootstrap requested its snapshot before subscribing, allowing events to be lost. Concurrent initialization could subscribe twice. | Subscribe first, buffer/replay events during bootstrap, share initialization, and unsubscribe on failure before a retry. |
| A8 | Medium | Event-loss recovery sent a complete list as an additive event, leaving deleted rows behind. Retained old deltas and late progress could revive stale state. | Use an authoritative snapshot event, discard the lagged receiver's old backlog, refresh queues/categories/settings, and prune missing rows, progress, selection, and anchors. |
| A9 | Medium | Firefox's recent-handoff map checked expiration only while recording another handoff. After an idle period, the same URL could be skipped indefinitely. | Check and remove expired entries when reading them, while retaining immediate duplicate suppression. |

### A1: credentials belong to one origin

The HTTP client strips credentials when it follows a redirect. The downloader then caches
that final URL and issues further range requests directly to it. Those direct requests
previously copied cookies and could regenerate Basic/Digest authentication, bypassing the
redirect client's protection. A configured mirror was also treated as a trusted recipient.

The credential origin is now immutable for the running HTTP context, including when its
primary URL is refreshed. Automatic credential sharing with a different mirror origin is
removed. Ordinary headers such as the user agent and referrer retain their existing behavior.

The regressions use two local servers on different ports. They verify both successful
multi-range downloads without leaked credentials and rejection of a foreign Basic challenge.
Existing same-origin Basic/Digest tests continue to pass.

### A2–A3: publishing a file is the commit point

Received data is flushed and closed before verification. A checksum mismatch now returns
the `.prismpart` path without replacing an existing destination. Valid downloads keep their
existing rename/conflict-policy behavior.

Cancel takes precedence over an earlier pause snapshot. A cancel that requests deletion
removes the partial file; cancellation without deletion retains it. The checksum reader
checks cancellation between 1 MiB chunks. Already-running operating-system reads or flushes
must return before the next cancellation check can run.

Tests cover preserving a pre-existing destination on a bad checksum, pause-to-cancel
upgrades, cancellation before publication with both deletion modes, and interruption of
the checksum loop after it has started reading.

### A4: malformed servers must fail within a bounded number of requests

An invalid first partial response may fall back to a plain GET for compatibility. A second
invalid partial response now fails instead of requesting forever. A local socket fixture
verifies that exactly two requests occur. Range-parser tests include out-of-total bounds,
zero totals, and an overflowing exclusive end.

### A5–A6: browser cookie context

Cookies scoped to `/files/private` must not be reused for `/files/public` just because the
URLs share a directory. Separate tests check exact-path caching and query-only reuse.

When a private store is unavailable, the extension does not query the regular store. Any
Cookie header captured from the private browser request is retained. A resolved private
store continues to work normally.

### A7–A8: consistent frontend recovery

Bootstrap waits for the listener to be installed, then fetches the snapshot while buffering
events. It replays those events before declaring the view ready. A failed bootstrap removes
its listener, allowing a clean retry.

`itemsAdded` remains incremental. The new `itemsSnapshot` event replaces the item map and
removes progress/selection for missing rows. The shell subscribes to a fresh receiver before
taking the recovery snapshot, so it does not replay the retained stale backlog afterwards.
Late progress for an unknown or already removed item is ignored.

### A9: Firefox duplicate suppression

The regression uses the real Firefox capture handlers with mocked browser/native APIs.
It verifies that an immediate duplicate is suppressed and a download of the same URL after
31 seconds is handed off and cancelled in the browser as expected.

## Test infrastructure

The Chromium/native-host integration harness also needed Linux isolation fixes:

- Register the native host in a temporary XDG configuration root and expose its manifest
  through the browser's temporary user-data directory.
- Give Chromium an explicit writable download directory in its preferences. Its normal
  download pipeline is used to exercise capture hooks, so a CDP download override alone
  does not set that destination.
- Keep test registration and cleanup away from the user's real browser configuration.

These changes address test-environment failures that initially appeared as a missing host
or interrupted browser downloads. The integration harness uses Chromium and the real native
host with a mock desktop backend.

## Validation

Focused regression runs before the fixes failed 13 tests across the Rust and TypeScript
suites. Additional checks cover cancellation during hashing, malformed-response fallback,
and final-file publication.

| Check | Result |
| --- | --- |
| `cargo test --locked --workspace --exclude prism-app` | 332 passed, 13 existing tests ignored, no failures |
| `cargo clippy --locked --workspace --exclude prism-app --all-targets -- -D warnings` | Passed |
| `cargo fmt --all -- --check` | Passed |
| Application tests and TypeScript check | 79 passed; typecheck passed |
| Extension tests and TypeScript check | 62 passed; typecheck passed |
| Application production build and both extension builds | Passed |
| Chromium fullscreen regressions | All three scenarios passed |
| Browser UI smoke test | Dashboard, removed-route fallback, and capture view passed |
| `npm run e2e --prefix extension` with Playwright Chromium and the real native host | 15/15 passed with a mock desktop backend |

The native Tauri shell check was attempted again and remains blocked by missing system
GLib development files (`glib-2.0.pc`). Native Windows focus behavior, Windows filesystem
execution, and real Firefox browser integration remain separate verification tasks. The
Firefox capture changes are covered by handler-level tests and the Firefox build.

## Release recommendations

1. **Treat checksum validation and origin isolation as release requirements.** Keep the new
   regressions in the normal Rust/TypeScript suites and require them for future changes to
   downloads, mirrors, or credential handling.
2. **Run a Windows build and NTFS pause/resume/cancel smoke test before distributing an
   executable.** The Linux checks establish behavior of the shared paths; they do not execute
   Windows-specific APIs or native window management.
3. **Configure a fork-owned updater endpoint and signing key before enabling self-update.**
   The current updater still targets upstream, which can replace a personal build with the
   original application. Keep update checks disabled for personal builds until that is set up.
4. **Exercise Firefox's real download pipeline before publishing an extension release.**
   Include repeated-URL captures and private browsing alongside the Chromium integration tests.
