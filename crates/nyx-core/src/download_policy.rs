//! DownloadGuard — politique pure pour le pre-flight d'un téléchargement.
//!
//! Pure logic, zero I/O. Le browser appelle `analyze(&Request)` au signal
//! WebKit `decide-destination` ; il reçoit un `Report` détaillé (verdict +
//! raisons + nom sanitizé) qu'il peut afficher tel quel dans le dialog.
//!
//! Ne décide JAMAIS sur l'extension seule : combine
//! `(filename + MIME + initiator origin + final origin + size + mode)`.
//! Voir docs/security.md §2 « Downloads ».

use crate::domain_risk::{analyze_url, Risk};
use crate::sec_log::{self, Level};

/// Mode global du navigateur — partagé via [`crate::mode`]. Ré-exporté pour
/// préserver la voie d'import `download_policy::Mode` historique.
pub use crate::mode::Mode;

// ─── Modèle ─────────────────────────────────────────────────────────────────

/// Verdict final — granularité riche pour ne pas confondre un .zip de github
/// avec un .exe d'un homographe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Type sûr depuis origine sûre → télécharger silencieusement.
    Allow,
    /// Confirmation utilisateur standard (archive, doc, type inconnu).
    Ask,
    /// Confirmation utilisateur **forte** : UI doit afficher un avertissement
    /// agressif et rendre le bouton "télécharger quand même" plus difficile.
    AskDanger,
    /// Refus sec — l'UI ne propose même pas d'override.
    Block,
}

/// Catégorie de fichier pour l'UI / les logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Safe,
    Executable,
    /// Archive — peut contenir n'importe quoi.
    Archive,
    /// HTML, SVG, etc. — pas exécutable mais peut embarquer du contenu actif.
    ActiveDoc,
    /// Office avec macros activées (`.docm`, `.xlsm`, `.pptm`).
    MacroDoc,
    Unknown,
}

/// Entrée du guard. Construite par le browser à partir des données WebKit.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// Filename suggéré par le serveur (`Content-Disposition`) ou WebKit.
    pub suggested_filename: &'a str,
    /// `Content-Type` brut.
    pub mimetype: &'a str,
    /// Page qui a initié le téléchargement (peut différer de `final_url`).
    pub initiator_url: &'a str,
    /// URL effective après tous les redirects — la VRAIE source du fichier.
    pub final_url: &'a str,
    /// Taille annoncée par `Content-Length` (None si absente).
    pub content_length: Option<u64>,
    pub mode: Mode,
}

impl<'a> Request<'a> {
    /// Constructeur "réseau direct" : initiator = final.
    pub fn direct(filename: &'a str, mime: &'a str, url: &'a str) -> Self {
        Self {
            suggested_filename: filename,
            mimetype: mime,
            initiator_url: url,
            final_url: url,
            content_length: None,
            mode: Mode::Normal,
        }
    }
}

/// Sortie : verdict + raisons + nom sanitizé. L'UI lit ça tel quel.
#[derive(Debug, Clone)]
pub struct Report {
    pub verdict: Verdict,
    pub kind: Kind,
    pub reasons: Vec<Reason>,
    /// Nom **sûr** à passer à `WebsiteDataManager::set_destination`.
    /// Toujours non-vide, jamais `.`/`..`, jamais de séparateur, ≤ 255 chars.
    pub normalized_filename: String,
    pub normalized_extension: Option<String>,
    pub initiator_origin: String,
    pub final_origin: String,
}

/// Raisons individuelles — combinables. Sert aux logs + à l'UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    SafeType,
    ExecutableExt,
    ArchiveType,
    ActiveContent,
    MacroEnabledDoc,
    UnknownType,
    DoubleExtension { hidden_ext: String, real_ext: String },
    BidiInFilename,
    NullByteInFilename,
    PathTraversal,
    WindowsReservedName,
    EmptyFilename,
    TooLongFilename,
    DangerousOrigin,
    SuspiciousOrigin,
    HttpDownload,
    MimeMismatch { ext_kind: Kind, mime_kind: Kind },
    OctetStreamMime,
    SizeUnknown,
    SizeHuge,
    SizeZero,
    InitiatorMismatch,
    BankingModeRestriction,
    ShadowModeRestriction,
}

// ─── API publique ──────────────────────────────────────────────────────────

/// Analyse complète. Pure : pas d'I/O, déterministe.
pub fn analyze(req: &Request) -> Report {
    let mut reasons = Vec::new();

    // 1. Sanitize le filename (peut être hostile : path traversal, bidi, NUL).
    let (normalized_filename, file_reasons) = sanitize_filename(req.suggested_filename);
    reasons.extend(file_reasons);

    let normalized_extension = extension_of(&normalized_filename).map(str::to_string);

    // 2. Classifie par extension/MIME et détecte les doubles extensions.
    let ext_kind = classify_extension(normalized_extension.as_deref());
    let mime_kind = classify_mime(req.mimetype);
    let double_ext = detect_double_extension(&normalized_filename);
    if let Some((hidden, real)) = &double_ext {
        reasons.push(Reason::DoubleExtension {
            hidden_ext: hidden.clone(), real_ext: real.clone(),
        });
    }

    // Le `Kind` retenu privilégie l'extension réelle (dernière) ; la cachée
    // (avant-dernière) est juste un signal supplémentaire.
    let kind = if ext_kind != Kind::Unknown { ext_kind } else { mime_kind };
    reasons.push(reason_for_kind(kind));

    // 3. Détecte les MIME mismatch (.png + application/x-msdownload, etc.).
    if ext_kind != Kind::Unknown && mime_kind != Kind::Unknown && ext_kind != mime_kind {
        reasons.push(Reason::MimeMismatch { ext_kind, mime_kind });
    }
    if req.mimetype.eq_ignore_ascii_case("application/octet-stream") {
        reasons.push(Reason::OctetStreamMime);
    }

    // 4. Origines initiator + final.
    let initiator_origin = origin_of(req.initiator_url);
    let final_origin     = origin_of(req.final_url);
    if !initiator_origin.is_empty() && !final_origin.is_empty()
        && initiator_origin != final_origin
    {
        reasons.push(Reason::InitiatorMismatch);
    }

    // 5. Risque d'origine (homographe, mixed-script…) sur le final.
    let final_risk = analyze_url(req.final_url).map(|a| a.risk);
    match final_risk {
        Some(Risk::Dangerous) => reasons.push(Reason::DangerousOrigin),
        Some(Risk::Suspicious) => reasons.push(Reason::SuspiciousOrigin),
        _ => {}
    }
    if req.final_url.starts_with("http://") && !is_localhost(&final_origin) {
        reasons.push(Reason::HttpDownload);
    }

    // 6. Politique de taille.
    match req.content_length {
        None if matches!(kind, Kind::Archive | Kind::Executable | Kind::Unknown)
            => reasons.push(Reason::SizeUnknown),
        Some(0) => reasons.push(Reason::SizeZero),
        Some(n) if n > 1_000_000_000 => reasons.push(Reason::SizeHuge),
        Some(n) if n > 100_000_000 && matches!(kind, Kind::Archive)
            => reasons.push(Reason::SizeHuge),
        _ => {}
    }

    // 7. Restrictions de mode (Banking, Shadow).
    apply_mode_restrictions(req.mode, kind, &mut reasons);

    // 8. Synthèse → verdict.
    let verdict = synthesize_verdict(kind, &reasons, req.mode);

    let report = Report {
        verdict, kind, reasons,
        normalized_filename, normalized_extension,
        initiator_origin, final_origin,
    };
    log(&report, req);
    report
}

// ─── Synthèse verdict ──────────────────────────────────────────────────────

/// Combine kind + raisons + mode → verdict final. Centralise toutes les
/// décisions hard pour éviter les conflits entre règles dispersées.
fn synthesize_verdict(kind: Kind, reasons: &[Reason], mode: Mode) -> Verdict {
    // Refus secs : origine dangereuse, traversée de path, NUL byte, MIME mismatch sévère.
    if reasons.iter().any(|r| matches!(r,
        Reason::DangerousOrigin
        | Reason::PathTraversal
        | Reason::NullByteInFilename
    )) {
        return Verdict::Block;
    }
    // MIME mismatch sévère : extension safe annoncée mais MIME exe → mensonge serveur.
    if reasons.iter().any(|r| matches!(r,
        Reason::MimeMismatch { ext_kind: Kind::Safe, mime_kind: Kind::Executable }
    )) {
        return Verdict::Block;
    }
    // Banking : tout sauf safe est bloqué.
    if matches!(mode, Mode::Banking) && !matches!(kind, Kind::Safe) {
        return Verdict::Block;
    }

    // AskDanger : double extension, bidi, exécutable, macro, HTTP+exec…
    let danger_signals = reasons.iter().any(|r| matches!(r,
        Reason::DoubleExtension { .. }
        | Reason::BidiInFilename
        | Reason::MacroEnabledDoc
        | Reason::SuspiciousOrigin
    ));
    if danger_signals { return Verdict::AskDanger; }
    if matches!(kind, Kind::Executable | Kind::MacroDoc) { return Verdict::AskDanger; }
    if matches!(kind, Kind::Executable) && reasons.iter().any(|r| matches!(r, Reason::HttpDownload)) {
        return Verdict::AskDanger;
    }

    // Ask : archive, type actif (HTML/SVG), inconnu, taille bizarre, mismatch initiator.
    if matches!(kind, Kind::Archive | Kind::ActiveDoc | Kind::Unknown) { return Verdict::Ask; }
    if reasons.iter().any(|r| matches!(r,
        Reason::SizeUnknown | Reason::SizeHuge | Reason::SizeZero
        | Reason::InitiatorMismatch | Reason::HttpDownload
        | Reason::MimeMismatch { .. } | Reason::OctetStreamMime
        | Reason::WindowsReservedName | Reason::EmptyFilename | Reason::TooLongFilename
        | Reason::ShadowModeRestriction
    )) {
        return Verdict::Ask;
    }

    Verdict::Allow
}

fn apply_mode_restrictions(mode: Mode, kind: Kind, reasons: &mut Vec<Reason>) {
    match mode {
        Mode::Banking if !matches!(kind, Kind::Safe) => reasons.push(Reason::BankingModeRestriction),
        Mode::Shadow if !matches!(kind, Kind::Safe) => reasons.push(Reason::ShadowModeRestriction),
        _ => {}
    }
}

fn reason_for_kind(k: Kind) -> Reason {
    match k {
        Kind::Safe       => Reason::SafeType,
        Kind::Executable => Reason::ExecutableExt,
        Kind::Archive    => Reason::ArchiveType,
        Kind::ActiveDoc  => Reason::ActiveContent,
        Kind::MacroDoc   => Reason::MacroEnabledDoc,
        Kind::Unknown    => Reason::UnknownType,
    }
}

// ─── Filename sanitizer ────────────────────────────────────────────────────

/// Nettoie un filename hostile. Retourne le nom safe + les raisons détectées.
/// Garantit : non-vide, pas de NUL, pas de `/` ni `\`, ≤ 255 chars, pas `.`/`..`.
fn sanitize_filename(raw: &str) -> (String, Vec<Reason>) {
    let mut reasons = Vec::new();

    // Ne garde que la dernière composante après séparateur (path traversal).
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    if last != raw {
        reasons.push(Reason::PathTraversal);
    }

    // NUL byte = refus sec côté verdict.
    if last.contains('\0') {
        reasons.push(Reason::NullByteInFilename);
    }

    // Caractères de contrôle bidi Unicode (U+202E RLO, U+202D LRO, U+2066-2069).
    if last.chars().any(is_bidi_control) {
        reasons.push(Reason::BidiInFilename);
    }

    // Retire NUL + contrôles + bidi, et trim les espaces de bord.
    let mut cleaned: String = last.chars()
        .filter(|c| *c != '\0' && !c.is_control() && !is_bidi_control(*c))
        .collect();
    cleaned = cleaned.trim().to_string();

    // `.` ou `..` : refus.
    if cleaned == "." || cleaned == ".." {
        reasons.push(Reason::PathTraversal);
        cleaned.clear();
    }

    if cleaned.is_empty() {
        reasons.push(Reason::EmptyFilename);
        cleaned = "download".to_string();
    }

    if cleaned.len() > 255 {
        reasons.push(Reason::TooLongFilename);
        cleaned.truncate(200);
    }

    // Noms réservés Windows (CON, PRN, AUX, NUL, COM1-9, LPT1-9) : on préfixe.
    if is_windows_reserved(&cleaned) {
        reasons.push(Reason::WindowsReservedName);
        cleaned = format!("_{cleaned}");
    }

    (cleaned, reasons)
}

fn is_bidi_control(c: char) -> bool {
    matches!(c as u32, 0x202A..=0x202E | 0x2066..=0x2069)
}

fn is_windows_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    matches!(stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL"
        | "COM1" | "COM2" | "COM3" | "COM4" | "COM5"
        | "COM6" | "COM7" | "COM8" | "COM9"
        | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5"
        | "LPT6" | "LPT7" | "LPT8" | "LPT9"
    )
}

/// Dernière extension (sans le point), insensible casse. None si pas d'ext.
fn extension_of(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() { return None; }
    Some(ext)
}

/// Détecte `safe.ext + dangerous.ext` (`facture.pdf.exe`, `photo.jpg.desktop`).
/// Retourne (extension cachée par le serveur, extension réelle).
fn detect_double_extension(name: &str) -> Option<(String, String)> {
    let (stem, real_ext) = name.rsplit_once('.')?;
    let (_, hidden_ext) = stem.rsplit_once('.')?;
    if hidden_ext.is_empty() || real_ext.is_empty() { return None; }

    let hidden_lower = hidden_ext.to_ascii_lowercase();
    let real_lower   = real_ext.to_ascii_lowercase();

    // Cas spécial : `tar.gz` etc. ne sont PAS des doubles extensions hostiles.
    if hidden_lower == "tar" { return None; }

    let hidden_kind = classify_extension(Some(hidden_lower.as_str()));
    let real_kind   = classify_extension(Some(real_lower.as_str()));

    // Hostile = la cachée semble safe à l'œil, la vraie est exécutable/archive.
    if matches!(hidden_kind, Kind::Safe | Kind::ActiveDoc | Kind::MacroDoc)
       && matches!(real_kind, Kind::Executable | Kind::Archive)
    {
        return Some((hidden_lower, real_lower));
    }
    None
}

// ─── Classification ────────────────────────────────────────────────────────

fn classify_extension(ext: Option<&str>) -> Kind {
    let Some(e) = ext else { return Kind::Unknown };
    let e = e.to_ascii_lowercase();
    if EXECUTABLE_EXTS.contains(&e.as_str()) { return Kind::Executable; }
    if ARCHIVE_EXTS.contains(&e.as_str())    { return Kind::Archive; }
    if MACRO_DOC_EXTS.contains(&e.as_str())  { return Kind::MacroDoc; }
    if ACTIVE_DOC_EXTS.contains(&e.as_str()) { return Kind::ActiveDoc; }
    if SAFE_EXTS.contains(&e.as_str())       { return Kind::Safe; }
    Kind::Unknown
}

fn classify_mime(mime: &str) -> Kind {
    let mime = mime.to_ascii_lowercase();
    let prefix = mime.split('/').next().unwrap_or("");
    match prefix {
        "image" | "audio" | "video" | "font" => Kind::Safe,
        "text" if mime == "text/html" => Kind::ActiveDoc,
        "text" => Kind::Safe,
        "application" => match mime.as_str() {
            "application/pdf" | "application/json" | "application/xml" => Kind::Safe,
            "application/zip" | "application/x-7z-compressed"
            | "application/x-rar-compressed" | "application/x-tar"
            | "application/gzip" | "application/x-bzip2" => Kind::Archive,
            "application/x-executable" | "application/x-msdownload"
            | "application/x-msi" | "application/vnd.debian.binary-package"
            | "application/x-sh" | "application/x-shellscript"
            | "application/vnd.android.package-archive"
            | "application/x-msdos-program" => Kind::Executable,
            "application/vnd.ms-word.document.macroenabled.12"
            | "application/vnd.ms-excel.sheet.macroenabled.12"
            | "application/vnd.ms-powerpoint.presentation.macroenabled.12" => Kind::MacroDoc,
            "application/xhtml+xml" => Kind::ActiveDoc,
            _ => Kind::Unknown,
        },
        _ => Kind::Unknown,
    }
}

fn origin_of(url: &str) -> String {
    let s = url.trim();
    let (scheme, default_port) = if s.starts_with("https://") { ("https", "443") }
        else if s.starts_with("http://") { ("http", "80") }
        else { return String::new() };
    let after = &s[scheme.len() + 3..];
    let end = after.find('/').unwrap_or(after.len());
    let authority = after[..end].rsplit('@').next().unwrap_or(&after[..end]);
    let host = match authority.rsplit_once(':') {
        Some((h, p)) if p == default_port => h,
        _ => authority,
    };
    if host.is_empty() { String::new() } else { format!("{scheme}://{host}") }
}

fn is_localhost(origin: &str) -> bool {
    origin.contains("//localhost") || origin.contains("//127.")
        || origin.contains("//[::1]")
}

fn log(report: &Report, req: &Request) {
    let level = match report.verdict {
        Verdict::Allow     => Level::Allow,
        Verdict::Ask       => Level::Warn,
        Verdict::AskDanger => Level::Warn,
        Verdict::Block     => Level::Block,
    };
    sec_log::emit(level, &format!(
        "download {:?} {:?}: {} ← {}",
        report.verdict, report.kind,
        report.normalized_filename,
        sec_log::redact_url(req.final_url),
    ));
}

// ─── Listes d'extensions ───────────────────────────────────────────────────

const EXECUTABLE_EXTS: &[&str] = &[
    // Windows
    "exe", "msi", "msix", "bat", "cmd", "com", "scr", "ps1", "vbs", "vbe",
    "js", "jse", "wsf", "wsh", "hta", "cpl", "pif", "reg", "lnk", "url",
    // Linux
    "deb", "rpm", "appimage", "snap", "flatpakref", "desktop", "service",
    "timer", "run", "bin",
    // macOS
    "dmg", "pkg", "app", "command", "workflow",
    // Scripts / runtimes
    "sh", "bash", "zsh", "py", "rb", "pl", "php", "jar",
    // Mobile
    "apk", "ipa", "xapk",
];

const ARCHIVE_EXTS: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst",
    "iso", "img", "cab", "lzh", "arj",
];

const MACRO_DOC_EXTS: &[&str] = &[
    "docm", "xlsm", "pptm", "dotm", "xltm", "potm",
];

const ACTIVE_DOC_EXTS: &[&str] = &[
    "html", "htm", "svg", "xhtml", "xht",
];

const SAFE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "ico",
    "mp4", "webm", "mkv", "mov", "avi", "m4v",
    "mp3", "ogg", "flac", "wav", "opus", "m4a",
    "pdf", "txt", "md", "json", "xml", "csv", "tsv", "log", "yaml", "yml",
    "css",
    "woff", "woff2", "ttf", "otf",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn req(filename: &'static str, mime: &'static str, url: &'static str) -> Request<'static> {
        Request::direct(filename, mime, url)
    }

    // ─── Cas nominaux ──────────────────────────────────────────────────

    #[test]
    fn safe_image_from_safe_origin_allows() {
        let r = analyze(&req("photo.jpg", "image/jpeg", "https://example.com/p"));
        assert_eq!(r.verdict, Verdict::Allow);
        assert_eq!(r.kind, Kind::Safe);
    }

    #[test]
    fn pdf_allow() {
        let r = analyze(&req("doc.pdf", "application/pdf", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::Allow);
    }

    #[test]
    fn executable_asks_danger() {
        let r = analyze(&req("setup.exe", "application/x-msdownload", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::AskDanger);
        assert_eq!(r.kind, Kind::Executable);
    }

    #[test]
    fn linux_desktop_asks_danger() {
        let r = analyze(&req("autostart.desktop", "", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::AskDanger);
    }

    #[test]
    fn archive_asks() {
        let r = analyze(&req("a.zip", "application/zip", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::Ask);
        assert_eq!(r.kind, Kind::Archive);
    }

    #[test]
    fn macro_doc_asks_danger() {
        let r = analyze(&req("invoice.docm", "", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::AskDanger);
        assert_eq!(r.kind, Kind::MacroDoc);
    }

    #[test]
    fn html_active_doc_asks() {
        let r = analyze(&req("page.html", "text/html", "https://example.com/"));
        assert_eq!(r.kind, Kind::ActiveDoc);
        assert_eq!(r.verdict, Verdict::Ask);
    }

    #[test]
    fn svg_active_doc_asks() {
        let r = analyze(&req("icon.svg", "image/svg+xml", "https://example.com/"));
        assert_eq!(r.kind, Kind::ActiveDoc);
    }

    // ─── Filename sanitizer ───────────────────────────────────────────

    #[test]
    fn empty_filename_normalized() {
        let r = analyze(&req("", "", "https://example.com/"));
        assert_eq!(r.normalized_filename, "download");
        assert!(r.reasons.contains(&Reason::EmptyFilename));
    }

    #[test]
    fn path_traversal_blocked() {
        let r = analyze(&req("../../evil.sh", "", "https://example.com/"));
        assert_eq!(r.normalized_filename, "evil.sh");
        assert!(r.reasons.contains(&Reason::PathTraversal));
    }

    #[test]
    fn null_byte_blocks() {
        let r = analyze(&req("evil\0.exe", "", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::Block);
        assert!(r.reasons.contains(&Reason::NullByteInFilename));
        assert!(!r.normalized_filename.contains('\0'));
    }

    #[test]
    fn bidi_unicode_flagged_as_danger() {
        // U+202E = Right-to-Left Override — inverse visuellement le texte.
        // Classique : `report.pdf<U+202E>cod.exe` apparaît `report.pdfexe.doc`.
        let r = analyze(&req("report.pdf\u{202E}cod.exe", "", "https://example.com/"));
        assert!(r.reasons.contains(&Reason::BidiInFilename));
        assert_eq!(r.verdict, Verdict::AskDanger);
        assert!(!r.normalized_filename.contains('\u{202E}'));
    }

    #[test]
    fn newline_in_filename_stripped() {
        let r = analyze(&req("evil\nContent-Type.txt", "", "https://example.com/"));
        assert!(!r.normalized_filename.contains('\n'));
    }

    #[test]
    fn windows_reserved_names_prefixed() {
        for name in ["CON", "PRN", "AUX", "NUL", "COM1", "LPT1", "con.txt"] {
            let r = analyze(&req(name, "", "https://example.com/"));
            assert!(r.normalized_filename.starts_with('_'),
                "{name} → {}", r.normalized_filename);
            assert!(r.reasons.contains(&Reason::WindowsReservedName));
        }
    }

    #[test]
    fn too_long_truncated() {
        let long = "a".repeat(400) + ".txt";
        let r = analyze(&req(Box::leak(long.into_boxed_str()), "", "https://example.com/"));
        assert!(r.normalized_filename.len() <= 200);
        assert!(r.reasons.contains(&Reason::TooLongFilename));
    }

    #[test]
    fn dot_and_dotdot_replaced() {
        for s in [".", ".."] {
            let r = analyze(&req(s, "", "https://example.com/"));
            assert_eq!(r.normalized_filename, "download");
            assert!(r.reasons.contains(&Reason::PathTraversal));
        }
    }

    // ─── Double extension ────────────────────────────────────────────

    #[test]
    fn double_extension_pdf_exe_asks_danger() {
        let r = analyze(&req("invoice.pdf.exe", "", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::AskDanger);
        assert!(r.reasons.iter().any(|x| matches!(x, Reason::DoubleExtension { .. })));
    }

    #[test]
    fn double_extension_jpg_desktop_asks_danger() {
        let r = analyze(&req("photo.jpg.desktop", "", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::AskDanger);
    }

    #[test]
    fn double_extension_txt_sh_asks_danger() {
        let r = analyze(&req("readme.txt.sh", "", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::AskDanger);
    }

    #[test]
    fn tar_gz_is_not_double_extension() {
        // `archive.tar.gz` est UNE archive, pas une attaque.
        let r = analyze(&req("a.tar.gz", "", "https://example.com/"));
        assert!(!r.reasons.iter().any(|x| matches!(x, Reason::DoubleExtension { .. })));
        assert_eq!(r.kind, Kind::Archive);
    }

    // ─── Origine ─────────────────────────────────────────────────────

    #[test]
    fn dangerous_origin_blocks_even_safe_file() {
        let r = analyze(&req("photo.jpg", "image/jpeg", "http://pаypal.com/img"));
        assert_eq!(r.verdict, Verdict::Block);
        assert!(r.reasons.contains(&Reason::DangerousOrigin));
    }

    #[test]
    fn suspicious_idn_asks_danger() {
        let r = analyze(&req("doc.pdf", "application/pdf", "https://bücher.de/x"));
        assert_eq!(r.verdict, Verdict::AskDanger);
        assert!(r.reasons.contains(&Reason::SuspiciousOrigin));
    }

    #[test]
    fn http_executable_asks_danger() {
        let r = analyze(&req("setup.exe", "", "http://example.com/"));
        assert_eq!(r.verdict, Verdict::AskDanger);
        assert!(r.reasons.contains(&Reason::HttpDownload));
    }

    #[test]
    fn http_localhost_safe() {
        let r = analyze(&req("photo.jpg", "image/jpeg", "http://localhost:8000/"));
        assert!(!r.reasons.contains(&Reason::HttpDownload));
        assert_eq!(r.verdict, Verdict::Allow);
    }

    #[test]
    fn initiator_mismatch_flagged() {
        let req = Request {
            suggested_filename: "f.zip", mimetype: "application/zip",
            initiator_url: "https://safe.com/",
            final_url:     "https://cdn.unknown.io/f.zip",
            content_length: Some(1_000), mode: Mode::Normal,
        };
        let r = analyze(&req);
        assert!(r.reasons.contains(&Reason::InitiatorMismatch));
    }

    // ─── MIME mismatch ───────────────────────────────────────────────

    #[test]
    fn png_ext_but_exe_mime_blocks() {
        // Le serveur ment : extension safe, MIME exe → refus sec.
        let r = analyze(&req("photo.png", "application/x-msdownload", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::Block);
    }

    #[test]
    fn png_ext_with_octet_stream_asks() {
        let r = analyze(&req("photo.png", "application/octet-stream", "https://example.com/"));
        assert_eq!(r.verdict, Verdict::Ask);
        assert!(r.reasons.contains(&Reason::OctetStreamMime));
    }

    #[test]
    fn html_mime_with_jpg_ext_flagged() {
        let r = analyze(&req("photo.jpg", "text/html", "https://example.com/"));
        assert!(r.reasons.iter().any(|x| matches!(x, Reason::MimeMismatch { .. })));
    }

    // ─── Taille ──────────────────────────────────────────────────────

    #[test]
    fn unknown_size_archive_asks() {
        let r = analyze(&Request {
            suggested_filename: "a.zip", mimetype: "application/zip",
            initiator_url: "https://example.com/", final_url: "https://example.com/a",
            content_length: None, mode: Mode::Normal,
        });
        assert!(r.reasons.contains(&Reason::SizeUnknown));
    }

    #[test]
    fn huge_size_flagged() {
        let r = analyze(&Request {
            suggested_filename: "vid.mp4", mimetype: "video/mp4",
            initiator_url: "https://example.com/", final_url: "https://example.com/v",
            content_length: Some(2_000_000_000), mode: Mode::Normal,
        });
        assert!(r.reasons.contains(&Reason::SizeHuge));
    }

    #[test]
    fn zero_size_flagged() {
        let r = analyze(&Request {
            suggested_filename: "x.pdf", mimetype: "application/pdf",
            initiator_url: "https://example.com/", final_url: "https://example.com/",
            content_length: Some(0), mode: Mode::Normal,
        });
        assert!(r.reasons.contains(&Reason::SizeZero));
    }

    // ─── Modes ───────────────────────────────────────────────────────

    #[test]
    fn banking_mode_blocks_archive() {
        let r = analyze(&Request {
            suggested_filename: "a.zip", mimetype: "application/zip",
            initiator_url: "https://bank.com/", final_url: "https://bank.com/",
            content_length: Some(1000), mode: Mode::Banking,
        });
        assert_eq!(r.verdict, Verdict::Block);
    }

    #[test]
    fn banking_mode_allows_safe() {
        let r = analyze(&Request {
            suggested_filename: "statement.pdf", mimetype: "application/pdf",
            initiator_url: "https://bank.com/", final_url: "https://bank.com/",
            content_length: Some(1000), mode: Mode::Banking,
        });
        assert_eq!(r.verdict, Verdict::Allow);
    }

    #[test]
    fn shadow_mode_treats_archives_as_ask() {
        let r = analyze(&Request {
            suggested_filename: "a.zip", mimetype: "application/zip",
            initiator_url: "https://example.com/", final_url: "https://example.com/",
            content_length: Some(1000), mode: Mode::Shadow,
        });
        assert!(matches!(r.verdict, Verdict::Ask | Verdict::AskDanger));
        assert!(r.reasons.contains(&Reason::ShadowModeRestriction));
    }

    // ─── Sanity ──────────────────────────────────────────────────────

    #[test]
    fn case_insensitive_extension() {
        assert_eq!(analyze(&req("SETUP.EXE", "", "https://e.com/")).kind, Kind::Executable);
        assert_eq!(analyze(&req("photo.JPG", "", "https://e.com/")).kind, Kind::Safe);
    }

    #[test]
    fn unknown_extension_asks() {
        let r = analyze(&req("file.xyz", "", "https://example.com/"));
        assert_eq!(r.kind, Kind::Unknown);
        assert_eq!(r.verdict, Verdict::Ask);
    }

    /// Panic test : entrées hostiles, jamais de panic.
    #[test]
    fn malformed_doesnt_panic() {
        let long_name = "a".repeat(1000);
        let names: &[&str] = &["", ".", "..", "....", "name.", ".ext", "a..b",
                     "\0\n", "日本.exe", "../../evil", "evil\u{202E}txt.exe",
                     "CON", "NUL.txt", &long_name];
        let mimes = ["", "/", "application", "image/", "text/plain", "weird"];
        let urls = ["", "://", "http://", "https://", "javascript:alert(1)",
                    "https://example.com/", "file:///etc/passwd"];
        for n in names {
            for m in &mimes {
                for u in &urls {
                    let r = Request::direct(n, m, u);
                    let _ = analyze(&r);
                }
            }
        }
    }

    /// Stress : 10 000 analyses variées.
    #[test]
    fn stress_thousands_of_analyses() {
        let names = ["a.jpg", "b.exe", "c.zip", "d.tar.gz", "e.UNKNOWN",
                     "facture.pdf.exe", "photo.jpg.desktop", "doc.docm"];
        let mimes = ["image/jpeg", "application/x-msdownload",
                     "application/zip", "application/pdf",
                     "application/octet-stream", "text/html"];
        let urls = ["https://example.com/", "http://pаypal.com/",
                    "https://github.com/", "http://example.com/", "https://bücher.de/"];
        for i in 0..10_000 {
            let _ = analyze(&req(names[i % names.len()],
                                 mimes[i % mimes.len()],
                                 urls[i % urls.len()]));
        }
    }
}

