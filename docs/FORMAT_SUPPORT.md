# FileFlow format and transformation support

## Two different promises

FileFlow deliberately separates:

- **recognized**: intake can identify/classify the file and present it correctly;
- **executable**: a real local engine is wired to perform the advertised operation safely.

A format must not be shown as convertible merely because FileFlow recognizes its extension.

## Recognized families

### Images and camera formats

JPEG/JPG/JFIF, PNG/APNG, WebP, AVIF, HEIC/HEIF, TIFF, BMP, GIF, SVG, ICO/CUR/ICNS, JPEG XL, JPEG 2000, TGA, DDS, OpenEXR, HDR/RGBE, PBM/PGM/PPM/PNM/PAM, PCX/DCX, QOI, XCF, PSD/PSB, EPS, WMF/EMF and common RAW families including DNG, CR2/CR3, NEF/NRW, ARW/SRF/SR2, ORF, RAF, RW2, PEF, X3F, ERF, KDC, DCR, MOS and MEF.

Recognition is broad; actual decoding is delegated to libvips and ImageMagick and therefore follows the codecs installed with those engines. HEIC/HEIF and other non-native img2pdf inputs are normalized to PNG before PDF assembly.

### PDF and office

PDF, DOC/DOCX, ODT, RTF, Pages, WPD, TeX, XLS/XLSX/XLSM, ODS, CSV/TSV, Numbers, PPT/PPS/PPTX/PPSX, ODP and Keynote.

### Text and structured data

TXT, Markdown, RST, LOG, HTML/HTM, EML/MAIL, JSON, XML, YAML, TOML, JSONL/NDJSON, SQL, INI/CFG/CONF and Java-style properties.

### Archives and compressed streams

ZIP, 7Z, RAR, TAR, gzip/tgz, bzip2/tbz/tbz2, xz/txz, Zstandard (`.zst`, `.zstd`, `.tzst`), LZ4, CAB, ARJ, CPIO and ISO.

### Audio

MP3, WAV, AAC, M4A, FLAC, OGG, Opus, WMA, AIFF/AIF, ALAC, APE, AC3/EAC3, DTS, AMR and MIDI.

### Video

MP4/M4V, MOV, MKV, AVI, WebM, MPEG/MPG, WMV, FLV, 3GP, MTS/M2TS, TS, OGV, VOB, ASF, RM/RMVB and DV.

### Ebooks and comics

Recognized: EPUB, MOBI, AZW/AZW3, FB2, CBZ, CBR, CB7 and DjVu.

Direct ebook conversion in the current runtime is intentionally limited to **EPUB and FB2**, which are handed to Pandoc in sandbox mode. Other ebook/comic containers remain recognized for inspection and future dedicated adapters rather than being advertised as directly convertible.

## Executable local transformations

The runtime currently wires real execution for:

- image conversion/batch conversion, optimization and resizing via libvips;
- images -> PDF via img2pdf;
- extended/HEIC images -> PNG via libvips or ImageMagick -> PDF via img2pdf;
- HTML -> PDF via an isolated headless Chromium-compatible browser, with bounded JavaScript execution and network/DNS disabled;
- EML -> safe HTML -> PDF after MIME decoding and script/markup neutralization;
- PDF merge/split via qpdf;
- PDF compression via Ghostscript;
- PDF -> images/text via Poppler;
- PDF OCR via OCRmyPDF and image OCR via Tesseract;
- Office/OpenDocument -> PDF via LibreOffice;
- metadata inspect/remove via ExifTool;
- archive create/extract/inspect via 7-Zip with archive-safety preflight;
- audio/video conversion, compatibility output, compression, audio extraction and GIF generation via FFmpeg;
- light document/text conversions via Pandoc;
- EPUB/FB2 conversion via Pandoc sandbox mode;
- generated previews for unfamiliar images, HTML, EML, Office, text, EPUB/FB2 and video inputs;
- Zstandard compression/decompression via `zstd`;
- LZ4 compression/decompression via `lz4`.

Planner-only capabilities remain marked unavailable until an executor is wired.

## Fast lossless compression

### Zstandard

Use when the user wants a strong speed/ratio balance. FileFlow exposes simple profiles rather than raw command-line flags and bounds compression concurrency through the resource scheduler. A `.zst`/`.zstd`/`.tzst` stream is restored to a sensible original filename during decompression.

### LZ4

Use when throughput and very fast decompression are more important than maximum ratio. FileFlow exposes a normal fast profile and a higher-compression profile while keeping the operation lossless.

### Traditional archive containers

ZIP/7Z/TAR and related formats remain appropriate when multiple files/directories must be packaged together. Zstd and LZ4 also expose one-click `TAR.ZST` and `TAR.LZ4` actions: FileFlow builds a temporary TAR, compresses it, deletes the intermediate file and finalizes the result atomically.

## Output guarantees

- originals remain protected by default;
- result paths use conflict handling rather than silent overwrite;
- temporary outputs are finalized after successful execution;
- guided mode writes results to the user's configured FileFlow directory;
- advanced mode can choose same-folder/subfolder/custom-folder behavior;
- archive extraction is checked for traversal, links, extreme entry counts, unpacked size and suspicious ratios before extraction.
