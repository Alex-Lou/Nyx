//! DownloadGuard — classifie un téléchargement et rend un verdict.
//!
//! **Politique pure** : pas de WebKit, pas d'I/O. La couche browser appelle
//! `decide(filename, mimetype, source)` à chaque `download_started` WebKit et
//! exécute le verdict (allow silencieusement, demander confirmation, refuser).
//!
//! Règles (voir docs/security.md §2 « Downloads ») :
//! - Types neutres (image, vidéo, audio, pdf, texte) → `Allow`.
//! - Types exécutables (`.exe`, `.msi`, `.deb`, `.dmg`, `.AppImage`, `.desktop`,
//!   `.sh`, `.bat`, `.app`, `.apk`, `.jar`) → `Ask` (confirmation explicite).
//! - Archives (`.zip`, `.rar`, `.7z`, `.tar.*`) → `Ask` (peut contenir un exécutable).
//! - Origine `Dangerous` (IDN suspect) → `Block` même pour les types neutres.
//! - Extension inconnue → `Ask` par défaut (opt-in plutôt qu'opt-out).

use crate::domain_risk::{analyze_url, Risk};
use crate::sec_log::{self, Level};

/// Verdict pour un téléchargement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Type sûr → télécharger silencieusement.
    Allow,
    /// Demander confirmation explicite à l'utilisateur (origine + nom + type).
    Ask,
    /// Refuser sec (origine dangereuse, type interdit).
    Block,
}

/// Catégorie pour l'UI (label clair dans la confirmation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// image/vidéo/audio/document.
    Safe,
    /// Binaire / script lançable directement.
    Executable,
    /// Archive qui peut contenir un exécutable.
    Archive,
    /// Type inconnu — on demande par sécurité.
    Unknown,
}

/// Décide du sort d'un téléchargement.
///
/// `filename` : nom de fichier final tel que proposé par WebKit (peut être vide).
/// `mimetype` : Content-Type côté serveur (peut être vide / mensonger).
/// `source_url` : URL de la page qui a déclenché le DL — pour vérifier l'origine.
pub fn decide(filename: &str, mimetype: &str, source_url: &str) -> (Verdict, Kind) {
    let kind = classify(filename, mimetype);

    // Origine dangereuse (IDN homographe, mixed-script) → bloquer même un .png.
    if let Some(a) = analyze_url(source_url) {
        if a.risk == Risk::Dangerous {
            sec_log::emit(Level::Block,
                &format!("download from dangerous origin: {}", a.ascii_host));
            return (Verdict::Block, kind);
        }
    }

    let v = match kind {
        Kind::Safe       => Verdict::Allow,
        Kind::Executable => Verdict::Ask,
        Kind::Archive    => Verdict::Ask,
        Kind::Unknown    => Verdict::Ask,
    };

    let level = match v {
        Verdict::Allow => Level::Allow,
        Verdict::Ask   => Level::Warn,
        Verdict::Block => Level::Block,
    };
    sec_log::emit(level, &format!(
        "download {:?} {kind:?}: {} ({mimetype})",
        v, sec_log::redact_url(source_url),
    ));
    (v, kind)
}

/// Classifie par extension d'abord (plus fiable), puis MIME en fallback.
/// L'extension est insensible à la casse (`.EXE` = `.exe`).
fn classify(filename: &str, mimetype: &str) -> Kind {
    if let Some(ext) = extension(filename) {
        let e = ext.to_ascii_lowercase();
        if EXECUTABLE_EXTS.contains(&e.as_str()) { return Kind::Executable; }
        if ARCHIVE_EXTS.contains(&e.as_str())    { return Kind::Archive; }
        if SAFE_EXTS.contains(&e.as_str())       { return Kind::Safe; }
    }
    // Fallback : MIME. Plus permissif (le serveur peut mentir).
    let mime = mimetype.to_ascii_lowercase();
    let prefix = mime.split('/').next().unwrap_or("");
    match prefix {
        "image" | "audio" | "video" | "text" => Kind::Safe,
        "application" => match mime.as_str() {
            "application/pdf" => Kind::Safe,
            "application/json" | "application/xml" => Kind::Safe,
            "application/zip" | "application/x-7z-compressed"
            | "application/x-rar-compressed" | "application/x-tar"
            | "application/gzip" => Kind::Archive,
            "application/x-executable" | "application/x-msdownload"
            | "application/x-msi" | "application/vnd.debian.binary-package"
            | "application/x-sh" | "application/x-shellscript" => Kind::Executable,
            _ => Kind::Unknown,
        },
        _ => Kind::Unknown,
    }
}

/// Dernière composante après le dernier `.` (sans le point). `None` si pas
/// d'extension, ou si le nom commence par `.` (fichier caché sans ext).
fn extension(filename: &str) -> Option<&str> {
    let name = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() { return None; }
    // Gère les doubles extensions courantes (`.tar.gz`, `.tar.bz2`).
    if let Some((s2, ext2)) = stem.rsplit_once('.') {
        if !s2.is_empty() && ext2.eq_ignore_ascii_case("tar") {
            return Some(match ext.to_ascii_lowercase().as_str() {
                "gz" | "bz2" | "xz" | "zst" => "tar.gz", // tous classés comme archive tar
                _ => ext,
            });
        }
    }
    Some(ext)
}

/// Exécutables natifs (Win/Mac/Linux/Android) + scripts shell.
const EXECUTABLE_EXTS: &[&str] = &[
    "exe", "msi", "msix", "bat", "cmd", "com", "scr", "ps1",     // Windows
    "deb", "rpm", "appimage", "snap", "flatpakref", "desktop",   // Linux
    "dmg", "pkg", "app", "command",                              // macOS
    "sh", "bash", "zsh", "py", "rb", "pl", "jar",                // scripts/runtime
    "apk", "ipa",                                                // mobile
];

/// Archives — peuvent contenir n'importe quoi → confirmation user.
const ARCHIVE_EXTS: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "tar.gz",
    "iso", "img", "cab",
];

/// Types neutres dont l'ouverture ne lance pas de code.
const SAFE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "bmp", "ico",
    "mp4", "webm", "mkv", "mov", "avi",
    "mp3", "ogg", "flac", "wav", "opus", "m4a",
    "pdf", "txt", "md", "json", "xml", "csv", "tsv", "log",
    "html", "htm", "css", "js",
    "woff", "woff2", "ttf", "otf",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Petit helper : decide sur une source "safe" pour isoler la classification.
    fn d(name: &str, mime: &str) -> (Verdict, Kind) {
        decide(name, mime, "https://example.com/p")
    }

    #[test]
    fn safe_image() {
        assert_eq!(d("photo.jpg", "image/jpeg"), (Verdict::Allow, Kind::Safe));
        assert_eq!(d("a.PNG", "image/png"),       (Verdict::Allow, Kind::Safe));
    }

    #[test]
    fn safe_pdf() {
        assert_eq!(d("doc.pdf", "application/pdf"), (Verdict::Allow, Kind::Safe));
    }

    #[test]
    fn windows_executable_asks() {
        assert_eq!(d("setup.exe", "application/x-msdownload"),
                   (Verdict::Ask, Kind::Executable));
        assert_eq!(d("installer.MSI", ""), (Verdict::Ask, Kind::Executable));
    }

    #[test]
    fn linux_executable_asks() {
        assert_eq!(d("nyx.deb", "").0, Verdict::Ask);
        assert_eq!(d("app.AppImage", "").0, Verdict::Ask);
        assert_eq!(d("autostart.desktop", "").0, Verdict::Ask);
        assert_eq!(d("script.sh", "").0, Verdict::Ask);
    }

    #[test]
    fn macos_executable_asks() {
        assert_eq!(d("disk.dmg", "").0, Verdict::Ask);
        assert_eq!(d("app.pkg", "").0, Verdict::Ask);
    }

    #[test]
    fn jar_and_apk_ask() {
        assert_eq!(d("Minecraft.jar", "").0, Verdict::Ask);
        assert_eq!(d("app.apk", "").0, Verdict::Ask);
    }

    #[test]
    fn archive_asks_because_might_contain_exe() {
        assert_eq!(d("a.zip", "").1, Kind::Archive);
        assert_eq!(d("a.7z", "").1,  Kind::Archive);
        assert_eq!(d("a.tar.gz", "").1, Kind::Archive);
        assert_eq!(d("a.tar", "").1, Kind::Archive);
    }

    #[test]
    fn unknown_extension_asks() {
        let (v, k) = d("mystery.xyz", "");
        assert_eq!(v, Verdict::Ask);
        assert_eq!(k, Kind::Unknown);
    }

    #[test]
    fn mime_fallback_when_no_extension() {
        assert_eq!(d("download", "image/png").1, Kind::Safe);
        assert_eq!(d("download", "application/x-executable").1, Kind::Executable);
        assert_eq!(d("download", "application/zip").1, Kind::Archive);
    }

    #[test]
    fn dangerous_origin_blocks_even_image() {
        let v = decide("photo.jpg", "image/jpeg", "http://pаypal.com/img").0;
        assert_eq!(v, Verdict::Block);
    }

    #[test]
    fn dangerous_origin_blocks_executable() {
        let v = decide("setup.exe", "", "http://аррӏе.com/x").0;
        assert_eq!(v, Verdict::Block);
    }

    #[test]
    fn safe_origin_passes_image() {
        let v = decide("photo.jpg", "image/jpeg", "https://example.com/").0;
        assert_eq!(v, Verdict::Allow);
    }

    #[test]
    fn case_insensitive_extension() {
        assert_eq!(d("SETUP.EXE", "").1,    Kind::Executable);
        assert_eq!(d("photo.JPG", "").1,    Kind::Safe);
        assert_eq!(d("archive.ZIP", "").1,  Kind::Archive);
    }

    #[test]
    fn no_extension_no_mime() {
        // Vraiment rien → Unknown → Ask.
        let (v, k) = d("README", "");
        assert_eq!(v, Verdict::Ask);
        assert_eq!(k, Kind::Unknown);
    }

    #[test]
    fn hidden_dotfile_no_ext() {
        // `.bashrc` : pas d'extension réelle, juste le préfixe `.`.
        let (_, k) = d(".bashrc", "");
        assert_eq!(k, Kind::Unknown);
    }

    #[test]
    fn path_with_separator_in_name() {
        // WebKit donne parfois un chemin complet → on prend la dernière composante.
        assert_eq!(d("C:\\Users\\x\\setup.exe", "").1, Kind::Executable);
        assert_eq!(d("/tmp/photo.jpg", "").1, Kind::Safe);
    }

    /// Panic test : entrées malformées, jamais de panic.
    #[test]
    fn malformed_doesnt_panic() {
        for name in ["", ".", "..", "....", "name.", ".ext", "a..b", "日本.exe", "\0\n"] {
            for mime in ["", "application", "image/", "/", "weird"] {
                let _ = decide(name, mime, "https://example.com");
            }
        }
    }

    /// Stress : milliers d'appels avec entrées variées, doit rester rapide.
    #[test]
    fn stress_many_downloads() {
        let names = ["a.jpg", "b.exe", "c.zip", "d.tar.gz", "e.UNKNOWN", "f"];
        let mimes = ["image/jpeg", "application/x-msdownload", "application/zip",
                     "", "application/octet-stream"];
        let urls = ["https://example.com/", "http://pаypal.com/", "https://github.com/"];
        for i in 0..10_000 {
            let _ = decide(
                names[i % names.len()],
                mimes[i % mimes.len()],
                urls[i % urls.len()],
            );
        }
    }
}
