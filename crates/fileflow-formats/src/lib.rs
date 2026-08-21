use fileflow_domain::{DetectedFormat, DetectionConfidence, FormatFamily};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
struct ExtensionDescriptor {
    id: &'static str,
    mime: Option<&'static str>,
    family: FormatFamily,
    archive_container: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FormatRegistry;

impl FormatRegistry {
    pub fn detect(&self, path: &Path, sample: &[u8]) -> DetectedFormat {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());

        let by_extension = extension.as_deref().and_then(extension_descriptor);
        let by_structure = structured_descriptor(sample, extension.as_deref());
        let by_magic =
            infer::get(sample).map(|kind| magic_descriptor(kind.mime_type(), kind.extension()));

        // Container formats are the main source of false positives: DOCX/XLSX/PPTX,
        // ODF, EPUB and iWork all look like ZIP to a generic magic detector. Inspect
        // the container markers before falling back to the extension or infer crate.
        if let Some(structured) = by_structure {
            return structured;
        }

        if let (Some(ext), Some(magic)) = (by_extension, by_magic.as_ref()) {
            if ext.archive_container && magic.family == FormatFamily::Archive {
                return from_extension(extension, ext);
            }

            // When content and suffix disagree, trust actual bytes. A .pdf file that
            // contains JPEG bytes is an image, not a PDF.
            if magic.family != FormatFamily::Unknown && magic.family != ext.family {
                return magic.clone();
            }
        }

        if let Some(magic) = by_magic
            && magic.family != FormatFamily::Unknown
        {
            return magic;
        }

        if let Some(ext) = by_extension {
            return from_extension(extension, ext);
        }

        if let Some(text) = text_descriptor(sample, extension.clone()) {
            return text;
        }

        DetectedFormat::unknown(extension)
    }
}


fn structured_descriptor(sample: &[u8], extension: Option<&str>) -> Option<DetectedFormat> {
    if sample.starts_with(b"%PDF-") {
        return Some(detected("pdf", extension, "application/pdf", FormatFamily::Pdf));
    }
    if sample.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(detected("png", extension, "image/png", FormatFamily::Image));
    }
    if sample.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(detected("jpeg", extension, "image/jpeg", FormatFamily::Image));
    }
    if sample.starts_with(b"GIF87a") || sample.starts_with(b"GIF89a") {
        return Some(detected("gif", extension, "image/gif", FormatFamily::Image));
    }
    if sample.len() >= 12 && &sample[..4] == b"RIFF" && &sample[8..12] == b"WEBP" {
        return Some(detected("webp", extension, "image/webp", FormatFamily::Image));
    }
    if sample.len() >= 12 && &sample[4..8] == b"ftyp" {
        let brand = &sample[8..12];
        if matches!(brand, b"avif" | b"avis") {
            return Some(detected("avif", extension, "image/avif", FormatFamily::Image));
        }
        if matches!(brand, b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1") {
            return Some(detected("heic", extension, "image/heic", FormatFamily::Image));
        }
        if matches!(brand, b"M4A " | b"M4B " | b"M4P ") {
            return Some(detected("m4a", extension, "audio/mp4", FormatFamily::Audio));
        }
        if matches!(brand, b"qt  ") {
            return Some(detected("mov", extension, "video/quicktime", FormatFamily::Video));
        }
        if matches!(brand, b"isom" | b"iso2" | b"mp41" | b"mp42" | b"avc1" | b"dash") {
            return Some(detected("mp4", extension, "video/mp4", FormatFamily::Video));
        }
    }

    let zip_like = sample.starts_with(b"PK\x03\x04") || sample.starts_with(b"PK\x05\x06") || sample.starts_with(b"PK\x07\x08");
    if zip_like {
        let has = |needle: &[u8]| sample.windows(needle.len()).any(|window| window == needle);
        if has(b"word/") || has(b"word\\") {
            return Some(detected(
                "docx", extension,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                FormatFamily::Document,
            ));
        }
        if has(b"xl/") || has(b"xl\\") {
            return Some(detected(
                "xlsx", extension,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                FormatFamily::Spreadsheet,
            ));
        }
        if has(b"ppt/") || has(b"ppt\\") {
            return Some(detected(
                "pptx", extension,
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                FormatFamily::Presentation,
            ));
        }
        if has(b"application/epub+zip") {
            return Some(detected("epub", extension, "application/epub+zip", FormatFamily::Ebook));
        }
        if has(b"application/vnd.oasis.opendocument.text") {
            return Some(detected("odt", extension, "application/vnd.oasis.opendocument.text", FormatFamily::Document));
        }
        if has(b"application/vnd.oasis.opendocument.spreadsheet") {
            return Some(detected("ods", extension, "application/vnd.oasis.opendocument.spreadsheet", FormatFamily::Spreadsheet));
        }
        if has(b"application/vnd.oasis.opendocument.presentation") {
            return Some(detected("odp", extension, "application/vnd.oasis.opendocument.presentation", FormatFamily::Presentation));
        }
        // Generic ZIP remains an archive unless a known container extension can
        // refine it (for example iWork files which are ZIP-based too).
        if let Some(ext) = extension.and_then(extension_descriptor)
            && ext.archive_container
            && ext.family != FormatFamily::Archive
        {
            return Some(from_extension(extension.map(str::to_owned), ext));
        }
    }

    None
}

fn detected(
    id: &str,
    original_extension: Option<&str>,
    mime: &str,
    family: FormatFamily,
) -> DetectedFormat {
    DetectedFormat {
        id: id.into(),
        extension: original_extension.map(str::to_owned),
        mime_type: Some(mime.into()),
        family,
        confidence: DetectionConfidence::Magic,
    }
}

fn text_descriptor(sample: &[u8], extension: Option<String>) -> Option<DetectedFormat> {
    if !looks_like_text(sample) {
        return None;
    }
    let text = std::str::from_utf8(sample).ok()?.trim_start_matches('\u{feff}').trim_start();
    let (id, mime) = if (text.starts_with('{') && text.contains(':')) || text.starts_with('[') {
        ("json", "application/json")
    } else if text.starts_with("<?xml") || (text.starts_with('<') && text.contains('>')) {
        if text.to_ascii_lowercase().contains("<html") {
            ("html", "text/html")
        } else {
            ("xml", "application/xml")
        }
    } else {
        ("text", "text/plain")
    };
    Some(DetectedFormat {
        id: id.into(),
        extension,
        mime_type: Some(mime.into()),
        family: FormatFamily::Text,
        confidence: DetectionConfidence::Magic,
    })
}

fn from_extension(extension: Option<String>, descriptor: ExtensionDescriptor) -> DetectedFormat {
    DetectedFormat {
        id: descriptor.id.into(),
        extension,
        mime_type: descriptor.mime.map(str::to_owned),
        family: descriptor.family,
        confidence: DetectionConfidence::Extension,
    }
}

fn magic_descriptor(mime: &str, extension: &str) -> DetectedFormat {
    let family = if mime == "application/pdf" {
        FormatFamily::Pdf
    } else if mime.starts_with("image/") {
        FormatFamily::Image
    } else if mime.starts_with("audio/") {
        FormatFamily::Audio
    } else if mime.starts_with("video/") {
        FormatFamily::Video
    } else if is_archive_mime(mime) {
        FormatFamily::Archive
    } else if mime.starts_with("text/") {
        FormatFamily::Text
    } else {
        FormatFamily::Unknown
    };

    DetectedFormat {
        id: normalize_format_id(extension).into(),
        extension: Some(extension.to_ascii_lowercase()),
        mime_type: Some(mime.to_owned()),
        family,
        confidence: DetectionConfidence::Magic,
    }
}

fn normalize_format_id(extension: &str) -> &str {
    match extension {
        "jpg" | "jpeg" => "jpeg",
        "tif" | "tiff" => "tiff",
        "htm" | "html" => "html",
        other => other,
    }
}

fn looks_like_text(sample: &[u8]) -> bool {
    if sample.is_empty() || sample.contains(&0) {
        return false;
    }

    std::str::from_utf8(sample).is_ok()
}

fn is_archive_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/zip"
            | "application/gzip"
            | "application/x-7z-compressed"
            | "application/x-rar-compressed"
            | "application/x-tar"
            | "application/x-bzip2"
            | "application/x-xz"
            | "application/zstd"
            | "application/x-zstd"
            | "application/x-lz4"
    )
}

fn extension_descriptor(extension: &str) -> Option<ExtensionDescriptor> {
    let descriptor = match extension {
        "jpg" | "jpeg" => image("jpeg", "image/jpeg"),
        "png" => image("png", "image/png"),
        "webp" => image("webp", "image/webp"),
        "avif" => image("avif", "image/avif"),
        "heic" => image("heic", "image/heic"),
        "heif" => image("heif", "image/heif"),
        "tif" | "tiff" => image("tiff", "image/tiff"),
        "bmp" => image("bmp", "image/bmp"),
        "gif" => image("gif", "image/gif"),
        "svg" => image("svg", "image/svg+xml"),
        "ico" => image("ico", "image/x-icon"),
        "jxl" => image("jxl", "image/jxl"),
        "psd" | "psb" => image("photoshop", "image/vnd.adobe.photoshop"),
        "eps" => image("eps", "application/postscript"),
        "dng" | "cr2" | "cr3" | "nef" | "nrw" | "arw" | "srf" | "sr2" | "orf" | "raf" | "rw2"
        | "pef" | "x3f" | "erf" | "kdc" | "dcr" | "mos" | "mef" => image("raw", "image/x-raw"),

        "pdf" => simple("pdf", Some("application/pdf"), FormatFamily::Pdf),

        "doc" => document("doc", "application/msword", false),
        "docx" => document(
            "docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            true,
        ),
        "odt" => document("odt", "application/vnd.oasis.opendocument.text", true),
        "rtf" => document("rtf", "application/rtf", false),
        "pages" => document("pages", "application/x-iwork-pages-sffpages", true),
        "wpd" => document("wpd", "application/vnd.wordperfect", false),
        "tex" => document("tex", "application/x-tex", false),

        "xls" => spreadsheet("xls", "application/vnd.ms-excel", false),
        "xlsx" | "xlsm" => spreadsheet(
            "xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            true,
        ),
        "ods" => spreadsheet(
            "ods",
            "application/vnd.oasis.opendocument.spreadsheet",
            true,
        ),
        "csv" => spreadsheet("csv", "text/csv", false),
        "tsv" => spreadsheet("tsv", "text/tab-separated-values", false),
        "numbers" => spreadsheet("numbers", "application/x-iwork-numbers-sffnumbers", true),

        "ppt" | "pps" => presentation("ppt", "application/vnd.ms-powerpoint", false),
        "pptx" | "ppsx" => presentation(
            "pptx",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            true,
        ),
        "odp" => presentation(
            "odp",
            "application/vnd.oasis.opendocument.presentation",
            true,
        ),
        "key" => presentation("keynote", "application/x-iwork-keynote-sffkey", true),

        "txt" | "md" | "markdown" | "rst" | "log" => {
            simple("text", Some("text/plain"), FormatFamily::Text)
        }
        "html" | "htm" => simple("html", Some("text/html"), FormatFamily::Text),
        "json" => simple("json", Some("application/json"), FormatFamily::Text),
        "xml" => simple("xml", Some("application/xml"), FormatFamily::Text),
        "yaml" | "yml" => simple("yaml", Some("application/yaml"), FormatFamily::Text),
        "toml" => simple("toml", Some("application/toml"), FormatFamily::Text),
        "jsonl" | "ndjson" => simple("jsonl", Some("application/x-ndjson"), FormatFamily::Text),
        "sql" => simple("sql", Some("application/sql"), FormatFamily::Text),
        "ini" | "cfg" | "conf" | "properties" => {
            simple("config", Some("text/plain"), FormatFamily::Text)
        }

        "zip" => archive("zip", "application/zip"),
        "7z" => archive("7z", "application/x-7z-compressed"),
        "rar" => archive("rar", "application/vnd.rar"),
        "tar" => archive("tar", "application/x-tar"),
        "gz" | "tgz" => archive("gzip", "application/gzip"),
        "bz2" | "tbz" | "tbz2" => archive("bzip2", "application/x-bzip2"),
        "xz" | "txz" => archive("xz", "application/x-xz"),
        "zst" | "zstd" | "tzst" => archive("zstd", "application/zstd"),
        "lz4" => archive("lz4", "application/x-lz4"),
        "cab" => archive("cab", "application/vnd.ms-cab-compressed"),
        "arj" => archive("arj", "application/x-arj"),
        "cpio" => archive("cpio", "application/x-cpio"),
        "iso" => archive("iso", "application/x-iso9660-image"),

        "mp3" => audio("mp3", "audio/mpeg"),
        "wav" => audio("wav", "audio/wav"),
        "aac" => audio("aac", "audio/aac"),
        "m4a" => audio("m4a", "audio/mp4"),
        "flac" => audio("flac", "audio/flac"),
        "ogg" => audio("ogg", "audio/ogg"),
        "opus" => audio("opus", "audio/opus"),
        "wma" => audio("wma", "audio/x-ms-wma"),
        "aiff" | "aif" => audio("aiff", "audio/aiff"),
        "alac" => audio("alac", "audio/mp4"),
        "ape" => audio("ape", "audio/x-monkeys-audio"),
        "ac3" => audio("ac3", "audio/ac3"),
        "eac3" | "ec3" => audio("eac3", "audio/eac3"),
        "dts" => audio("dts", "audio/vnd.dts"),
        "amr" => audio("amr", "audio/amr"),
        "mid" | "midi" => audio("midi", "audio/midi"),

        "mp4" | "m4v" => video("mp4", "video/mp4"),
        "mov" => video("mov", "video/quicktime"),
        "mkv" => video("mkv", "video/x-matroska"),
        "avi" => video("avi", "video/x-msvideo"),
        "webm" => video("webm", "video/webm"),
        "mpeg" | "mpg" => video("mpeg", "video/mpeg"),
        "wmv" => video("wmv", "video/x-ms-wmv"),
        "flv" => video("flv", "video/x-flv"),
        "3gp" => video("3gp", "video/3gpp"),
        "mts" | "m2ts" | "ts" => video("mpeg-ts", "video/mp2t"),
        "ogv" => video("ogv", "video/ogg"),
        "vob" => video("vob", "video/mpeg"),
        "asf" => video("asf", "video/x-ms-asf"),
        "rm" | "rmvb" => video("realmedia", "application/vnd.rn-realmedia"),
        "dv" => video("dv", "video/dv"),

        "epub" => ebook("epub", "application/epub+zip", true),
        "mobi" => ebook("mobi", "application/x-mobipocket-ebook", false),
        "azw" | "azw3" => ebook("amazon-ebook", "application/vnd.amazon.ebook", false),
        "fb2" => ebook("fb2", "application/x-fictionbook+xml", false),
        "cbz" => ebook("cbz", "application/vnd.comicbook+zip", true),
        "cbr" => ebook("cbr", "application/vnd.comicbook-rar", false),
        "cb7" => ebook("cb7", "application/x-cb7", false),
        "djvu" | "djv" => ebook("djvu", "image/vnd.djvu", false),
        _ => return None,
    };

    Some(descriptor)
}

const fn simple(
    id: &'static str,
    mime: Option<&'static str>,
    family: FormatFamily,
) -> ExtensionDescriptor {
    ExtensionDescriptor {
        id,
        mime,
        family,
        archive_container: false,
    }
}

const fn image(id: &'static str, mime: &'static str) -> ExtensionDescriptor {
    simple(id, Some(mime), FormatFamily::Image)
}

const fn audio(id: &'static str, mime: &'static str) -> ExtensionDescriptor {
    simple(id, Some(mime), FormatFamily::Audio)
}

const fn video(id: &'static str, mime: &'static str) -> ExtensionDescriptor {
    simple(id, Some(mime), FormatFamily::Video)
}

const fn archive(id: &'static str, mime: &'static str) -> ExtensionDescriptor {
    simple(id, Some(mime), FormatFamily::Archive)
}

const fn document(
    id: &'static str,
    mime: &'static str,
    archive_container: bool,
) -> ExtensionDescriptor {
    ExtensionDescriptor {
        id,
        mime: Some(mime),
        family: FormatFamily::Document,
        archive_container,
    }
}

const fn spreadsheet(
    id: &'static str,
    mime: &'static str,
    archive_container: bool,
) -> ExtensionDescriptor {
    ExtensionDescriptor {
        id,
        mime: Some(mime),
        family: FormatFamily::Spreadsheet,
        archive_container,
    }
}

const fn presentation(
    id: &'static str,
    mime: &'static str,
    archive_container: bool,
) -> ExtensionDescriptor {
    ExtensionDescriptor {
        id,
        mime: Some(mime),
        family: FormatFamily::Presentation,
        archive_container,
    }
}

const fn ebook(
    id: &'static str,
    mime: &'static str,
    archive_container: bool,
) -> ExtensionDescriptor {
    ExtensionDescriptor {
        id,
        mime: Some(mime),
        family: FormatFamily::Ebook,
        archive_container,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_overrides_misleading_image_extension() {
        let registry = FormatRegistry;
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let detected = registry.detect(Path::new("renamed.jpg"), &png);

        assert_eq!(detected.family, FormatFamily::Image);
        assert_eq!(detected.id, "png");
        assert_eq!(detected.confidence, DetectionConfidence::Magic);
    }

    #[test]
    fn office_container_keeps_specific_extension_over_zip_magic() {
        let registry = FormatRegistry;
        let zip = [b'P', b'K', 0x03, 0x04, 0x14, 0x00, 0x00, 0x00];
        let detected = registry.detect(Path::new("report.docx"), &zip);

        assert_eq!(detected.family, FormatFamily::Document);
        assert_eq!(detected.id, "docx");
        assert_eq!(detected.confidence, DetectionConfidence::Extension);
    }

    #[test]
    fn pdf_magic_beats_misleading_extension() {
        let registry = FormatRegistry;
        let detected = registry.detect(Path::new("invoice.jpg"), b"%PDF-1.7\n%FileFlow");

        assert_eq!(detected.family, FormatFamily::Pdf);
        assert_eq!(detected.id, "pdf");
        assert_eq!(detected.confidence, DetectionConfidence::Magic);
    }

    #[test]
    fn ooxml_markers_identify_docx_even_with_wrong_suffix() {
        let registry = FormatRegistry;
        let sample = b"PK\x03\x04........word/document.xml........[Content_Types].xml";
        let detected = registry.detect(Path::new("mystery.zip"), sample);

        assert_eq!(detected.family, FormatFamily::Document);
        assert_eq!(detected.id, "docx");
        assert_eq!(detected.confidence, DetectionConfidence::Magic);
    }

    #[test]
    fn iso_bmff_brand_distinguishes_heic_from_generic_media() {
        let registry = FormatRegistry;
        let sample = b"\0\0\0\x18ftypheic\0\0\0\0heic";
        let detected = registry.detect(Path::new("phone.bin"), sample);

        assert_eq!(detected.family, FormatFamily::Image);
        assert_eq!(detected.id, "heic");
        assert_eq!(detected.confidence, DetectionConfidence::Magic);
    }

    #[test]
    fn structured_text_identifies_json_and_html() {
        let registry = FormatRegistry;
        let json = registry.detect(Path::new("payload.bin"), br#"{"ok":true}"#);
        let html = registry.detect(Path::new("page.bin"), b"<!doctype html><html><body>Hi</body></html>");

        assert_eq!(json.id, "json");
        assert_eq!(json.family, FormatFamily::Text);
        assert_eq!(html.id, "html");
        assert_eq!(html.family, FormatFamily::Text);
    }

    #[test]
    fn utf8_without_known_extension_is_text() {
        let registry = FormatRegistry;
        let detected = registry.detect(Path::new("README.unknown"), b"hello FileFlow\n");

        assert_eq!(detected.family, FormatFamily::Text);
        assert_eq!(detected.id, "text");
    }

    #[test]
    fn recognizes_modern_compression_and_raw_formats() {
        let registry = FormatRegistry;
        let compressed = registry.detect(Path::new("backup.tar.zst"), b"");
        let raw = registry.detect(Path::new("photo.CR3"), b"");
        let comic = registry.detect(Path::new("album.cbz"), b"");

        assert_eq!(compressed.family, FormatFamily::Archive);
        assert_eq!(compressed.id, "zstd");
        assert_eq!(raw.family, FormatFamily::Image);
        assert_eq!(raw.id, "raw");
        assert_eq!(comic.family, FormatFamily::Ebook);
        assert_eq!(comic.id, "cbz");
    }

    #[test]
    fn recognizes_extended_media_and_data_formats() {
        let registry = FormatRegistry;
        let audio = registry.detect(Path::new("track.eac3"), b"");
        let video = registry.detect(Path::new("capture.m2ts"), b"");
        let data = registry.detect(Path::new("events.ndjson"), b"{\"id\":1}\n");

        assert_eq!(audio.family, FormatFamily::Audio);
        assert_eq!(video.family, FormatFamily::Video);
        assert_eq!(data.family, FormatFamily::Text);
        assert_eq!(data.id, "jsonl");
    }
}
