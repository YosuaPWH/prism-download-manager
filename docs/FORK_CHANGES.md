# Personal fork changes

## Model downloads

The dedicated Hugging Face model/dataset downloader has been removed: toolbar/sidebar
entries, settings, model window/route, Tauri commands, model discovery, automatic token
loading, special URL handling and model queue creation. Old settings still deserialize
(unknown fields are ignored). Existing downloads and queues remain ordinary manager data.
Direct file URLs can still be downloaded through the general downloader.

## Video fullscreen

The hover button previously moved into the fullscreen player container; only fullscreen
`<video>` elements were excluded. The button and its quality menu are now suppressed for
any player fullscreen, including ancestor fullscreen around a cross-origin iframe.
Visibility returns on a fresh hover after exiting fullscreen. Frame messages contain only
a boolean visibility hint; they cannot request downloads or carry browser session data.

The Chromium regression runs independently of the native app:

```sh
npm ci --prefix extension
npx --prefix extension playwright install chromium
npm run e2e:fullscreen --prefix extension
```

To use this behavior, build and load **this fork's extension**, not the unchanged Web Store
copy. Run `npm run build --prefix extension`, remove the installed Prism extension (the
build retains its ID), then Load unpacked from `extension/build/chromium`. After rebuilding,
reload the extension and affected video tabs.

## Download windows

The IPC server starts before the desktop event subscriber is attached. A dialog request
can therefore precede the subscriber; a lagged broadcast receiver can also lose a request.
The shell now reconciles pending capture/batch/video requests every two seconds and opens
missing windows. Existing dialogs are not refocused by this reconciliation. Confirmed or
cancelled requests are removed and are not reopened. Native close actions (Alt+F4 or the
taskbar) cancel the pending request too, even when they bypass the React Cancel button.

Window-creation failures no longer silently confirm a capture and start its download with
defaults: the request stays pending for a retry. A failed progress-window open is recorded
as handled only after it succeeds, so a later reconciliation can retry it.

The progress window is enabled by default for new profiles. Existing saved preferences
are preserved. On an older profile, enable **Settings > General > Windows > Show the
Download File dialog** and **Open a progress window for new downloads** if desired.

Core regressions exercise a late subscriber and an overflowing event stream. The browser
preview verifies that the dashboard and capture UI render, but native Windows focus and
window-creation behavior still require a Windows smoke test.

## Upstream issue #70: sparse files

Source report: <https://github.com/FortoGraphy/prism-download-manager/issues/70>

The HTTP/FTP writer used its close operation both for completion and for pause/error.
That operation called `finish_file`, which cleared `FSCTL_SET_SPARSE` on Windows. On an
incomplete file, clearing sparse can materialize all missing ranges. This explains why a
small paused download could continue writing gigabytes and appear stuck. Cancellation
cannot interrupt an already-running synchronous filesystem control operation.

The writer no longer clears sparse when closing. Resuming also marks the partial file
sparse again, repairing legacy flags (it cannot undo disk space already allocated by an
older build). Unix uses sparse sizing rather than eager `fallocate`. Flushing received
buffers and syncing them for a durable resume point are retained; a pause can still need
a short disk flush. Filesystems without sparse support, such as FAT/exFAT, retain their
filesystem-dependent allocation behavior, with a diagnostic warning on Windows.

Torrent downloads use **librqbit 9.0.1's own filesystem storage**, which already marks
Windows files sparse and does not use Prism's `finish_file`. No identical flag-clearing
bug was found there, so this change does not claim to resolve every torrent symptom in
the report. A separate offline torrent regression checks that sizing, pausing and deleting
an incomplete torrent preserve holes and remove the file. The Windows branch of that
regression requires NTFS/ReFS and must be run on Windows.

Focused storage checks:

```sh
cargo test -p prism-engine closing_a_partial_file_preserves_sparse_allocation -- --nocapture
cargo test -p prism-engine sizing_and_resuming_a_large_file_preserves_holes
cargo test -p prism-torrent --test layout pausing_an_incomplete_torrent_does_not_materialize_missing_data
```

Before using a Windows release, build the installer as described in `BUILD.md`, then check:

1. With Prism initially closed, capture a download from the browser and verify that its
   save-location dialog opens. Repeat with Prism in the tray.
2. Start the download and verify the progress window opens when that setting is enabled.
   Cancel a capture and verify it stays dismissed after two seconds.
3. On an NTFS drive, start a large multi-connection download, pause after a few megabytes,
   then resume and cancel. Confirm there is no full-size zero-fill write after pausing.
4. Repeat the pause/delete check with a torrent; report any remaining disk writes separately
   because that path uses a different storage implementation.

## Validation in the Linux cloud workspace

| Check | Result |
| --- | --- |
| `cargo test --locked -p prism-core -p prism-engine -p prism-torrent` | 182 tests passed, including the doctest |
| `cargo clippy --locked -p prism-core -p prism-engine -p prism-torrent --all-targets -- -D warnings` | Passed |
| `cargo fmt --all -- --check` | Passed |
| App tests | 73 passed |
| Extension tests | 57 passed |
| App and extension TypeScript checks | Passed |
| App production build; Chromium/Firefox extension builds | Passed |
| Chromium fullscreen regressions | Player container, video element and cross-origin iframe passed |
| Browser UI smoke test | Dashboard, removed-route fallback and capture view passed without page errors |
| `cargo check --locked -p prism-app` | Blocked by missing system GLib development files (`glib-2.0.pc`); native shell not fully checked |

Windows build, native window behavior and the report's NTFS disk-usage reproduction remain
manual verification steps. Linux tests do not establish Windows-specific filesystem results.
