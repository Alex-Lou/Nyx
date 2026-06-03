//! magic_bytes — détection de type par signature octet.
//!
//! Pure : entrée `&[u8]`, sortie [`Detected`]. Aucun I/O, aucune allocation
//! sur le chemin chaud. Sert au **post-flight** du DownloadGuard : après
//! écriture du fichier temporaire, le browser sniff les premiers octets pour
//! vérifier que l'extension et le MIME annoncés ne mentent pas.
//!
//! Ce qu'il ne fait PAS :
//!   - aucun hashing (un `magic_bytes_hash` viendra si nécessaire) ;
//!   - aucun parsing complet (header uniquement, jamais le contenu) ;
//!   - aucun verdict — la sortie est mappée vers [`Kind`] via [`category`].
//!
//! Voir docs/security.md §2 « Downloads — post-flight ».

use crate::download_policy::Kind;

/// Type détecté par signature. `Unknown` pour buffers trop courts ou
/// non reconnus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detected {
    Png, Jpeg, Gif, Webp, Pdf,
    Zip, Rar, SevenZ, Tar, Gzip, Bz2,
    Elf, MachO, PeExe, Class, Wasm,
    Iso, Sqlite,
    Html, Svg, Xml, Json,
    Unknown,
}

/// Détecte le type d'un buffer. Sûr sur entrée vide ou tronquée.
pub fn sniff(bytes: &[u8]) -> Detected {
    if bytes.is_empty() { return Detected::Unknown; }

    // Signatures binaires courtes (ordre = priorité).
    if starts_with(bytes, &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Detected::Png;
    }
    if starts_with(bytes, &[0xFF, 0xD8, 0xFF]) {
        return Detected::Jpeg;
    }
    if is_gif(bytes)       { return Detected::Gif; }
    if is_webp(bytes)      { return Detected::Webp; }
    if starts_with(bytes, b"%PDF")           { return Detected::Pdf; }
    if is_zip(bytes)       { return Detected::Zip; }
    if starts_with(bytes, &[b'R', b'a', b'r', b'!', 0x1A, 0x07]) {
        return Detected::Rar;
    }
    if starts_with(bytes, &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return Detected::SevenZ;
    }
    if starts_with(bytes, &[0x1F, 0x8B]) { return Detected::Gzip; }
    if starts_with(bytes, b"BZh")        { return Detected::Bz2; }
    if starts_with(bytes, &[0x7F, b'E', b'L', b'F']) { return Detected::Elf; }
    if is_macho(bytes)  { return Detected::MachO; }
    if is_pe(bytes)     { return Detected::PeExe; }
    // CAFEBABE : Java class (et MachO Fat — on suit la spec et on retient Class).
    if starts_with(bytes, &[0xCA, 0xFE, 0xBA, 0xBE]) { return Detected::Class; }
    if starts_with(bytes, &[0x00, 0x61, 0x73, 0x6D]) { return Detected::Wasm; }
    if is_iso(bytes)    { return Detected::Iso; }
    if starts_with(bytes, b"SQLite format 3\0") { return Detected::Sqlite; }
    if is_tar(bytes)    { return Detected::Tar; }

    // Signatures texte (case-insensitive, après BOM + whitespace).
    let t = skip_ws_and_bom(bytes);
    if starts_with_ci(t, b"<!doctype html") || starts_with_ci(t, b"<html") {
        return Detected::Html;
    }
    if starts_with_ci(t, b"<svg") { return Detected::Svg; }
    if starts_with_ci(t, b"<?xml") {
        // SVG embarqué possible : on cherche "<svg" dans les 1 Kio suivants.
        let end = t.len().min(1024);
        if find_ci(&t[..end], b"<svg").is_some() {
            return Detected::Svg;
        }
        return Detected::Xml;
    }
    if matches!(t.first(), Some(b'{') | Some(b'[')) {
        return Detected::Json;
    }

    Detected::Unknown
}

/// Cohérence entre type sniffé et extension. Case-insensitive.
/// `matches_extension(PeExe, "png") == false` ⇒ MIME mismatch détecté.
pub fn matches_extension(sniffed: Detected, ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    extensions_for(sniffed).contains(&ext.as_str())
}

/// Mappe vers la [`Kind`] consommée par `download_policy`.
pub fn category(sniffed: Detected) -> Kind {
    match sniffed {
        Detected::Png | Detected::Jpeg | Detected::Gif | Detected::Webp
        | Detected::Pdf | Detected::Json | Detected::Xml | Detected::Sqlite
            => Kind::Safe,
        Detected::Elf | Detected::MachO | Detected::PeExe
        | Detected::Class | Detected::Wasm
            => Kind::Executable,
        Detected::Zip | Detected::Rar | Detected::SevenZ | Detected::Tar
        | Detected::Gzip | Detected::Bz2 | Detected::Iso
            => Kind::Archive,
        Detected::Html | Detected::Svg => Kind::ActiveDoc,
        Detected::Unknown              => Kind::Unknown,
    }
}

fn extensions_for(d: Detected) -> &'static [&'static str] {
    match d {
        Detected::Png    => &["png"],
        Detected::Jpeg   => &["jpg", "jpeg", "jpe"],
        Detected::Gif    => &["gif"],
        Detected::Webp   => &["webp"],
        Detected::Pdf    => &["pdf"],
        // ZIP container : docx/jar/apk sont tous des ZIP par construction.
        Detected::Zip    => &["zip", "jar", "apk", "ipa", "xapk",
                              "docx", "xlsx", "pptx", "odt", "ods", "odp", "epub"],
        Detected::Rar    => &["rar"],
        Detected::SevenZ => &["7z"],
        Detected::Tar    => &["tar"],
        Detected::Gzip   => &["gz", "tgz"],
        Detected::Bz2    => &["bz2", "tbz2"],
        Detected::Elf    => &["bin", "out", "so"],
        Detected::MachO  => &["dylib", "bin"],
        Detected::PeExe  => &["exe", "dll", "msi", "scr", "cpl", "sys"],
        Detected::Class  => &["class"],
        Detected::Wasm   => &["wasm"],
        Detected::Iso    => &["iso", "img"],
        Detected::Sqlite => &["sqlite", "sqlite3", "db"],
        Detected::Html   => &["html", "htm", "xhtml"],
        Detected::Svg    => &["svg"],
        Detected::Xml    => &["xml"],
        Detected::Json   => &["json"],
        Detected::Unknown => &[],
    }
}

// ─── Détecteurs spécifiques ────────────────────────────────────────────────

fn is_gif(b: &[u8]) -> bool {
    b.len() >= 6 && &b[..4] == b"GIF8"
        && matches!(b[4], b'7' | b'9') && b[5] == b'a'
}

fn is_webp(b: &[u8]) -> bool {
    b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP"
}

fn is_zip(b: &[u8]) -> bool {
    b.len() >= 4 && b[0] == 0x50 && b[1] == 0x4B
        && matches!((b[2], b[3]), (0x03, 0x04) | (0x05, 0x06) | (0x07, 0x08))
}

fn is_macho(b: &[u8]) -> bool {
    b.len() >= 4 && matches!(
        (b[0], b[1], b[2], b[3]),
        (0xCE, 0xFA, 0xED, 0xFE) | (0xCF, 0xFA, 0xED, 0xFE)
        | (0xFE, 0xED, 0xFA, 0xCE) | (0xFE, 0xED, 0xFA, 0xCF)
    )
}

fn is_pe(b: &[u8]) -> bool {
    if b.len() < 0x40 || &b[..2] != b"MZ" { return false; }
    let off = u32::from_le_bytes([b[0x3C], b[0x3D], b[0x3E], b[0x3F]]) as usize;
    // Garde-fou : offset doit rester dans le buffer fourni.
    off.checked_add(4).is_some_and(|end| end <= b.len() && &b[off..end] == b"PE\0\0")
}

fn is_iso(b: &[u8]) -> bool {
    // Volume Descriptor à 0x8001 (= 32769), 5 octets 'CD001'.
    b.len() >= 0x8006 && &b[0x8001..0x8006] == b"CD001"
}

fn is_tar(b: &[u8]) -> bool {
    // POSIX ustar magic à l'offset 257, 5 octets.
    b.len() >= 263 && &b[257..262] == b"ustar"
}

// ─── Helpers texte ─────────────────────────────────────────────────────────

fn starts_with(buf: &[u8], pfx: &[u8]) -> bool {
    buf.len() >= pfx.len() && &buf[..pfx.len()] == pfx
}

fn starts_with_ci(buf: &[u8], pfx: &[u8]) -> bool {
    buf.len() >= pfx.len()
        && buf[..pfx.len()].iter().zip(pfx).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn find_ci(buf: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || buf.len() < needle.len() { return None; }
    (0..=buf.len() - needle.len()).find(|&i| starts_with_ci(&buf[i..], needle))
}

fn skip_ws_and_bom(buf: &[u8]) -> &[u8] {
    let mut b = buf.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(buf);
    while let Some(c) = b.first() {
        if c.is_ascii_whitespace() { b = &b[1..]; } else { break; }
    }
    b
}

#[cfg(test)]
mod tests;
