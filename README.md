<p align="center">
  <img src="assets/brand/png/app-icon-256.png" width="100" alt="Prism">
</p>

<h1 align="center">Prism — Personal Fork</h1>

<p align="center">
  Fork oleh <a href="https://github.com/YosuaPWH">YosuaPWH</a> dari
  <a href="https://github.com/FortoGraphy/prism-download-manager">Prism Download Manager</a>.
  Berfokus pada pengalaman menonton, dialog download, dan penyimpanan file parsial.
</p>

## Perubahan dari aplikasi asli

Perbandingan ini mengacu pada baseline upstream **1.2.4**, commit
[`60898a7`](https://github.com/FortoGraphy/prism-download-manager/commit/60898a726f38172559aa29836fb7940e81176eab).

| Area | Perilaku pada baseline asli | Perubahan di fork ini |
| --- | --- | --- |
| Download AI model | Downloader khusus Hugging Face untuk mencari model/dataset, memilih file, dan memakai token. | Fitur khusus ini dihapus dari UI, pengaturan, API, dan pemrosesan URL. |
| Tombol video saat fullscreen | Tombol dapat muncul di dalam container player fullscreen. | Tombol dan menu kualitas disembunyikan selama fullscreen, termasuk fullscreen pada container yang membungkus iframe lintas domain. Tersedia kembali saat video di-hover setelah keluar fullscreen. |
| Dialog download | Permintaan dialog dapat terlewat saat startup atau ketika penerima event tertinggal. Kegagalan membuka dialog dapat langsung memulai download dengan pengaturan default. | Permintaan yang masih menunggu diperiksa ulang setiap dua detik; jendela yang belum terbuka dicoba lagi. Kegagalan membuka dialog tetap menunggu konfirmasi pengguna. |
| Jendela progress | Nonaktif secara default; kegagalan membuka jendela dapat tercatat sebagai sudah ditangani. | Aktif secara default untuk profil baru. Jendela yang gagal dibuka dapat dicoba lagi. Preferensi profil lama tetap dipertahankan. |
| Penyimpanan file parsial HTTP/FTP | Penutupan writer, termasuk saat pause/error, menghapus flag sparse di Windows. Linux memakai alokasi awal melalui `fallocate`. | Flag sparse dipertahankan saat file ditutup dan diterapkan lagi saat resume. Unix memakai ukuran file sparse agar ruang untuk bagian yang belum diterima tidak dialokasikan di awal. |

Menutup dialog lewat tombol Cancel, **Alt+F4**, atau taskbar juga membatalkan permintaan
terkait, sehingga mekanisme pemulihan tidak membukanya kembali. Dialog yang sudah terbuka
tidak difokuskan ulang oleh pemeriksaan berkala.

Penghapusan fitur AI model tidak menghapus riwayat download atau antrean yang sudah ada.
File model melalui URL file langsung tetap dapat diunduh memakai downloader umum.

Analisis teknis dan langkah verifikasi lengkap tersedia di
[docs/FORK_CHANGES.md](docs/FORK_CHANGES.md).

## Penanganan issue upstream #70

[Issue #70](https://github.com/FortoGraphy/prism-download-manager/issues/70) melaporkan
penulisan disk besar yang berlanjut setelah download di-pause atau dibatalkan pada Windows.

Fork ini memperbaiki jalur HTTP/FTP yang menghapus flag sparse saat menutup file parsial.
Menghapus flag tersebut dapat memicu alokasi bagian file yang belum terisi. Data yang sudah
diterima tetap di-flush untuk menjaga resume yang aman, sehingga pause masih bisa memerlukan
waktu singkat untuk menyelesaikan penulisan buffer.

Cakupan verifikasinya:

- Pengujian Linux memastikan file berukuran logis **10 GB** yang baru terisi beberapa byte
  tetap sparse setelah writer ditutup.
- Torrent memakai penyimpanan terpisah dari **librqbit**. Tes offline untuk pause dan delete
  sudah lolos di Linux, tetapi tidak ditemukan jalur penghapusan flag yang sama di sana.
- Reproduksi pada Windows/NTFS dan gejala torrent Windows masih memerlukan pengujian langsung.
  Perubahan ini belum membuktikan seluruh gejala dalam issue tersebut selesai.
- Filesystem tanpa dukungan sparse, seperti FAT/exFAT, tetap mengikuti perilaku alokasi
  filesystem tersebut. Mengaktifkan sparse saat resume tidak mengembalikan ruang yang sudah
  teralokasi oleh versi lama.

## Memakai enhancement fork ini

### Aplikasi desktop

Build aplikasi dari checkout yang sudah memuat perubahan fork ini. Installer atau binary
rilis upstream tetap berisi kode dari proyek aslinya.

Untuk profil lama, buka **Settings → General → Windows** dan aktifkan sesuai kebutuhan:

- **Show the Download File dialog** untuk memilih lokasi dan mengonfirmasi download.
- **Open a progress window for new downloads** untuk membuka jendela progress otomatis.

Konfigurasi updater saat ini masih mengarah ke rilis upstream. Untuk mempertahankan build
pribadi, nonaktifkan pemeriksaan update otomatis melalui Settings dan hindari memasang
update upstream sampai updater dikonfigurasi untuk rilis fork sendiri.

### Extension browser

Perubahan fullscreen membutuhkan **extension hasil build fork ini**. Extension resmi dari
Web Store tetap mengikuti perilaku versi resminya.

```sh
npm ci --prefix extension
npm run build --prefix extension
```

Untuk Chrome, Edge, Brave, dan browser Chromium lain:

1. Hapus extension Prism yang terpasang terlebih dahulu; build fork masih memakai ID yang sama.
2. Buka halaman extensions browser, lalu aktifkan **Developer mode**.
3. Pilih **Load unpacked** dan arahkan ke `extension/build/chromium`.
4. Jalankan aplikasi fork. Jika belum terhubung, buka **Settings → Browser integration → Repair**,
   lalu restart browser.
5. Setelah membangun ulang extension, reload extension dan tab video yang digunakan.

Untuk uji sementara di Firefox, buka `about:debugging#/runtime/this-firefox`, pilih
**Load Temporary Add-on**, lalu pilih `extension/build/firefox/manifest.json`.

## Build installer Windows tanpa GitHub Actions

Siapkan **Node.js 24 LTS**, **Rust stable ≥ 1.88 dengan toolchain MSVC**, serta
**Visual Studio 2022 Build Tools** dengan komponen *Desktop development with C++* dan
Windows SDK. Dari root checkout fork, jalankan di PowerShell Windows:

```powershell
npm ci --prefix app
npm ci --prefix extension
npm run build --prefix extension

cd app
npm run tauri:build -- --bundles nsis
```

Installer dihasilkan di:

```text
target\release\bundle\nsis\Prism_<versi>_x64-setup.exe
```

Build ini berjalan di komputer Windows sendiri dan tidak memakai menit GitHub Actions.
Panduan platform lainnya: [BUILD.md](BUILD.md) dan [build macOS](docs/BUILD_MACOS.md).

## Hasil pengujian perubahan

Validasi berikut dilakukan saat implementasi di cloud Linux:

| Pemeriksaan | Hasil |
| --- | --- |
| Tes Rust untuk core, engine, dan torrent, termasuk doctest | 182 lolos |
| Tes aplikasi | 73 lolos |
| Tes extension | 57 lolos |
| Clippy pada crate yang diubah dan format Rust | Lolos |
| Pemeriksaan tipe TypeScript aplikasi dan extension | Lolos |
| Build frontend dan extension Chromium/Firefox | Lolos |
| Tes fullscreen Chromium | Container player, elemen video, dan iframe lintas domain lolos |
| Pemeriksaan UI di browser | Dashboard, fallback route model yang dihapus, dan dialog capture lolos |

Pemeriksaan shell Tauri terhenti karena library pengembangan GLib (`glib-2.0.pc`) belum
tersedia di cloud. Build native, perilaku jendela Windows, dan reproduksi penggunaan disk
NTFS **belum diverifikasi langsung**. Detail perintah tes ada di
[catatan validasi](docs/FORK_CHANGES.md#validation-in-the-linux-cloud-workspace).

## Fondasi dari proyek asli

Fork ini tetap memakai download engine Rust, UI React/Tauri, native messaging untuk
integrasi browser, HTTP(S)/FTP, video dan stream, torrent/magnet, antrean, scheduler,
serta fitur manajemen download dari Prism.

Kredit pengembangan aplikasi asli tetap untuk **Jan Sechovec dan para kontributor Prism**.
Lihat [repo upstream](https://github.com/FortoGraphy/prism-download-manager) untuk proyek
aslinya dan [ARCHITECTURE.md](ARCHITECTURE.md) untuk arsitektur kode.

## Lisensi

Kode Prism memakai [lisensi MIT](LICENSE). Komponen pihak ketiga tetap mengikuti lisensi
masing-masing; lihat [THIRD_PARTY.md](THIRD_PARTY.md).
