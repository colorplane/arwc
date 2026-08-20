# compress-arw

Lossless compressor for uncompressed Sony ARW (14-bit, 16-bit aligned). Encoded files use the **`.ARWC.JPG`** extension: they are JPEGs (camera preview + EXIF first, compressed raw after `FF D9`).

Previewers, browsers, and Finder stop at JPEG EOI and show the image. `exiftool` reads the JPEG APP1 EXIF. The raw is not needed to view or to extract the JPEG.

**Demo:** [arwc.colorplane.com](https://arwc.colorplane.com/)

Sony stores Orientation on the ARW TIFF IFD0, not on the camera JPEG. Encode therefore prepends a small **view APP1** (`Software=ARWC`) with Orientation and, when one exists, a copied IFD1 JPEG thumbnail. Browsers and Gallery can rotate and thumbnail from the first tens of kilobytes. Decode **strips** that APP1 before splicing the camera JPEG back into the ARW, so the restored file is bit-identical. Files encoded before this APP1 still decode.

## Cameras

The codec does not look at the model name. It accepts Sony ARW whose raw strip is **uncompressed 14-bit**, 16-bit aligned, at EOF — the camera setting **RAW File Type → Uncompressed**. Sony’s lossy “Compressed RAW” and lossless-compressed ARW 4.0 are refused.

**Tested:** Sony α7R III (ILCE-7RM3). On that body: **MENU → Camera Settings 1 → RAW File Type → Uncompressed**. Single-shot and continuous stay 14-bit. BULB and Long Exposure NR drop to 12-bit and will be refused.

Other bodies that can write the same format (when Uncompressed is selected and the file stays 14-bit) include α7R II–VI, α7 II / III / IV, α7C / C II / CR, α7S II / III, α9 / II / III, α1 / II, α99 II, RX1R II / III, and ILX-LR1.

Does not work: original α7 / α7R / α7S and most APS-C (compressed RAW only); α6700 (lossless compressed, no uncompressed option); 12-bit files from BULB, Long Exposure NR, or silent shooting on some older bodies.

## Downloads

From [arwc.colorplane.com](https://arwc.colorplane.com) (v0.1.0):

- [Windows x86_64 app](https://arwc.colorplane.com/arwc-0.1.0.exe) (ARWC viewer with compressor bundled)
- [macOS Apple Silicon app](https://arwc.colorplane.com/arwc-0.1.0.dmg) (ARWC viewer with compressor bundled)

## Build on Linux

Native CLI only (no WASM). You need a C compiler (for zstd) and [Rust](https://rustup.rs/).

Debian / Ubuntu:

```
sudo apt update
sudo apt install -y build-essential
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Fedora:

```
sudo dnf install -y gcc
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Then, from this repository:

```
cargo build --release --bin compress-arw
```

The binary is `target/release/compress-arw`. Example:

```
./target/release/compress-arw encode shot.ARW
./target/release/compress-arw decode shot.ARWC.JPG
```

## Layout

```
[SOI]
[ARWC view APP1: Orientation + optional copied IFD1 JPEG]  ← display-only; stripped on decode
[camera JPEG with original EXIF … EOI]
[ARWZ trailer: TIFF remainder + zstd(Bayer-delta + byte-shuffle of the raw strip)]
[orig size u64 LE + SHA-1 of the original ARW + magic ARWH]
```

| Want | What to read |
|---|---|
| Orientation / EXIF thumbnail | First ~64 KiB (the view APP1). HTTP `Range` works. |
| View / full preview JPEG | Bytes `0 .. EOI` (typically < 1 MiB). |
| Original ARW | Whole file → `decode` (strips the view APP1) |

The trailing TIFF header stores everything except the preview JPEG (that JPEG is already at the front). Decode splices it back so the ARW is bit-identical. The last 32 bytes store the uncompressed ARW size and its SHA-1 (`ARWH`); decode checks both.

## Library

```rust
use compress_arw::{decode, encode, extract_preview};

let jpg = encode(&arw_bytes)?;          // save as .ARWC.JPG
let jpeg = extract_preview(&jpg)?;      // no zstd, no raw
let arw = decode(&jpg)?;                // original ARW
```

`encode_with_level(&arw, 9)` if you encode in WASM (faster, ~2 MiB larger than level 19).

## CLI

```
compress-arw encode  OCT00527.ARW                 # → OCT00527.ARWC.JPG
compress-arw preview OCT00527.ARWC.JPG -o preview.jpg
compress-arw decode  OCT00527.ARWC.JPG -o OCT00527.ARW
compress-arw info    OCT00527.ARWC.JPG
```

## WASM

```
wasm-pack build --release --target web -- --no-default-features --features wasm
```

Exports: `encode(arw, level)`, `decode(file)`, `extract_preview(file)`, `preview_bytes(file)`.

## Electron viewer

Opens a folder of `.ARW` / `.ARWC.JPG` files, shows the camera JPEG, compresses and decompresses, and moves between files with the arrow keys.

```
cd electron
npm install
npm start
```

`npm start` builds `compress-arw` in release mode first. Shortcuts: `←` `→` browse, `C` compress, `D` decompress, `O` open folder.

Packaged builds bundle the same CLI (`extraResources`; on Windows also next to `ARWC.exe`):

```
npm run dist:mac    # macOS DMG (run on Apple Silicon)
npm run dist:win    # Windows NSIS installer (run on Windows)
```

## Tests

Synthetic ARWs are generated at runtime. Real-camera fixtures come from [colorplane/CompressARWTestData](https://github.com/colorplane/CompressARWTestData) and are checked with SHA-256: encode then decode must match the original file hash.

```
git clone --depth 1 https://github.com/colorplane/CompressARWTestData testdata
cargo test --test realdata
```

Or point `COMPRESS_ARW_TESTDATA` at a clone of that repo.

GitHub Actions (free for public repositories) clones that fixture repo and runs the same tests on push and pull request. It also stores versioned artifacts: the Windows Electron installer (`arwc-0.1.0.exe`, CLI bundled) and the macOS CLI (`compress-arw-0.1.0-macos-aarch64`).

## License

[MIT](LICENSE)
