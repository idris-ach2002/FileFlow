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
        let by_magic =
            infer::get(sample).map(|kind| magic_descriptor(kind.mime_type(), kind.extension()));

        if let (Some(ext), Some(magic)) = (by_extension, by_magic.as_ref()) {
            if ext.archive_container && magic.family == FormatFamily::Archive {
                return from_extension(extension, ext);
            }

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

        if looks_like_text(sample) {
            return DetectedFormat {
                id: "text".into(),
                extension,
                mime_type: Some("text/plain".into()),
                family: FormatFamily::Text,
                confidence: DetectionConfidence::Magic,
            };
        }

        DetectedFormat::unknown(extension)
    }
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
        "dng" | "cr2" | "cr3" | "nef" | "arw" | "orf" | "raf" | "rw2" => {
            image("raw", "image/x-raw")
        }

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

        "zip" => archive("zip", "application/zip"),
        "7z" => archive("7z", "application/x-7z-compressed"),
        "rar" => archive("rar", "application/vnd.rar"),
        "tar" => archive("tar", "application/x-tar"),
        "gz" | "tgz" => archive("gzip", "application/gzip"),
        "bz2" => archive("bzip2", "application/x-bzip2"),
        "xz" => archive("xz", "application/x-xz"),
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

        "epub" => ebook("epub", "application/epub+zip", true),
        "mobi" => ebook("mobi", "application/x-mobipocket-ebook", false),
        "azw" | "azw3" => ebook("amazon-ebook", "application/vnd.amazon.ebook", false),
        "fb2" => ebook("fb2", "application/x-fictionbook+xml", false),
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
    fn utf8_without_known_extension_is_text() {
        let registry = FormatRegistry;
        let detected = registry.detect(Path::new("README.unknown"), b"hello FileFlow\n");

        assert_eq!(detected.family, FormatFamily::Text);
        assert_eq!(detected.id, "text");
    }
}
