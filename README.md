<p align="center">
  <img src="assets/brand/png/app-icon-256.png" width="100" alt="Prism">
</p>

<h1 align="center">Prism — Personal Fork</h1>

<p align="center">
  A personal fork by <a href="https://github.com/YosuaPWH">YosuaPWH</a> of
  <a href="https://github.com/FortoGraphy/prism-download-manager">Prism Download Manager</a>.
  Focused on video playback, download dialogs, and partial-file storage.
</p>

## Changes from the original application

This comparison uses upstream **1.2.4**, commit
[`60898a7`](https://github.com/FortoGraphy/prism-download-manager/commit/60898a726f38172559aa29836fb7940e81176eab),
as its baseline.

| Area | Original baseline behavior | Changes in this fork |
| --- | --- | --- |
| AI model downloads | A dedicated Hugging Face downloader for discovering models/datasets, selecting files, and using access tokens. | Remove the dedicated feature from the UI, settings, API, and special URL handling. |
| Video button during fullscreen | The button can appear inside a fullscreen player container. | Hide the button and quality menu during fullscreen, including fullscreen containers around cross-origin iframes. Restore availability on a fresh hover after exiting fullscreen. |
| Download dialogs | Dialog requests can be missed during startup or when the event receiver falls behind. A failed dialog open can start the download immediately with default settings. | Reconcile pending requests every two seconds and retry missing windows. Keep a failed dialog request pending for user confirmation. |
| Progress windows | Disabled by default; a failed window open can be recorded as already handled. | Enable progress windows by default for new profiles and retry failed opens. Preserve existing profiles' saved preferences. |
| HTTP/FTP partial-file storage | Closing the writer, including on pause/error, clears the sparse flag on Windows. Linux reserves space using `fallocate`. | Keep the sparse flag when closing and reapply it on resume. Use sparse sizing on Unix so missing ranges are not allocated in advance. |

Closing a dialog through Cancel, **Alt+F4**, or the taskbar also cancels its pending request,
so reconciliation does not reopen it. Periodic reconciliation does not refocus dialogs that
are already open.

Removing the dedicated AI model downloader preserves existing download history and queues.
Direct model-file URLs can still be downloaded through the general downloader.

For technical analysis and detailed verification steps, see
[docs/FORK_CHANGES.md](docs/FORK_CHANGES.md).

## Addressing upstream issue #70

[Issue #70](https://github.com/FortoGraphy/prism-download-manager/issues/70) reports heavy
disk writes continuing after a download is paused or cancelled on Windows.

This fork fixes the HTTP/FTP path that cleared the sparse flag when closing a partial file.
Clearing that flag can cause missing ranges to be allocated. Received data is still flushed
for a durable resume point, so pausing may briefly wait for buffered writes to finish.

Verification scope:

- Linux tests confirm that a file with a logical size of **10 GB**, containing only a few
  received bytes, remains sparse after the writer closes.
- Torrents use **librqbit's** separate storage implementation. Its offline pause/delete test
  passes on Linux; the same sparse-flag-clearing path was not found there.
- The Windows/NTFS reproduction and Windows torrent symptoms still require direct testing.
  These changes do not establish that every symptom in the report is resolved.
- Filesystems without sparse support, such as FAT/exFAT, retain their filesystem-dependent
  allocation behavior. Reapplying sparse on resume does not reclaim space already allocated
  by an older build.

## Using this fork's enhancements

### Desktop application

Build the application from a checkout containing this fork's changes. Upstream release
installers and binaries contain the original project's code.

For existing profiles, open **Settings → General → Windows** and enable these as needed:

- **Show the Download File dialog** to choose a save location and confirm each download.
- **Open a progress window for new downloads** to open progress windows automatically.

The updater configuration still points to upstream releases. To keep a personal build,
disable automatic update checks in Settings and avoid installing upstream updates until
the updater is configured for this fork's own releases.

### Browser extension

The fullscreen changes require **an extension built from this fork**. The official Web Store
extension retains the behavior of its published version.

```sh
npm ci --prefix extension
npm run build --prefix extension
```

For Chrome, Edge, Brave, and other Chromium browsers:

1. Remove the installed Prism extension first; this fork's build retains the same ID.
2. Open the browser's extensions page and enable **Developer mode**.
3. Select **Load unpacked** and choose `extension/build/chromium`.
4. Start the forked application. If it does not connect, open
   **Settings → Browser integration → Repair**, then restart the browser.
5. After rebuilding the extension, reload it and the video tabs you are using.

For temporary testing in Firefox, open `about:debugging#/runtime/this-firefox`, select
**Load Temporary Add-on**, and choose `extension/build/firefox/manifest.json`.

## Building a Windows installer without GitHub Actions

Install **Node.js 24 LTS**, **Rust stable ≥ 1.88 with the MSVC toolchain**, and
**Visual Studio 2022 Build Tools** with *Desktop development with C++* and the Windows SDK.
From the root of your fork checkout, run these commands in Windows PowerShell:

```powershell
npm ci --prefix app
npm ci --prefix extension
npm run build --prefix extension

cd app
npm run tauri:build -- --bundles nsis
```

The installer is generated at:

```text
target\release\bundle\nsis\Prism_<version>_x64-setup.exe
```

This build runs on your own Windows computer and does not consume GitHub Actions minutes.
For other platforms, see [BUILD.md](BUILD.md) and the [macOS build guide](docs/BUILD_MACOS.md).

## Validation results

The following checks ran during implementation in the Linux cloud workspace:

| Check | Result |
| --- | --- |
| Rust tests for core, engine, and torrent, including the doctest | 182 passed |
| Application tests | 73 passed |
| Extension tests | 57 passed |
| Clippy for the affected crates and Rust formatting | Passed |
| Application and extension TypeScript checks | Passed |
| Frontend and Chromium/Firefox extension builds | Passed |
| Chromium fullscreen regressions | Player container, video element, and cross-origin iframe passed |
| Browser UI smoke test | Dashboard, removed-model-route fallback, and capture dialog passed |

The Tauri shell check was blocked by missing GLib development files (`glib-2.0.pc`) in the
cloud environment. Native builds, Windows window behavior, and the original NTFS disk-usage
reproduction **have not been verified directly**. For the exact test commands, see the
[validation notes](docs/FORK_CHANGES.md#validation-in-the-linux-cloud-workspace).

## Foundation from the original project

This fork retains Prism's Rust download engine, React/Tauri UI, native messaging for browser
integration, HTTP(S)/FTP downloads, video and streaming support, torrents/magnet links,
queues, scheduler, and download management features.

Credit for the original application belongs to **Jan Sechovec and the Prism contributors**.
See the [upstream repository](https://github.com/FortoGraphy/prism-download-manager) for the
original project and [ARCHITECTURE.md](ARCHITECTURE.md) for the code architecture.

## License

Prism's own code is licensed under [MIT](LICENSE). Third-party components retain their
respective licenses; see [THIRD_PARTY.md](THIRD_PARTY.md).
