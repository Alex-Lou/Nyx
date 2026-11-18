//! Plan d'un téléchargement — pur (l'existence des fichiers est injectée).
//!
//! WebView2 garde la main sur le téléchargement (UI native, SmartScreen,
//! Mark-of-the-Web). Nyx se contente de faire passer le nom suggéré par
//! `nyx_core::download_policy` (bidi, NUL, noms réservés, points finaux…) et
//! de refuser les verdicts `Block` (origine dangereuse, mensonge MIME).

use std::path::{Path, PathBuf};

use nyx_core::download_policy::{self, Request, Verdict};
use nyx_core::url;

/// Ce que la coquille doit faire du téléchargement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Laisser WebView2 écrire à `path` (changé seulement si `Some`).
    Save { name: String, path: Option<PathBuf> },
    /// Annuler (verdict `Block` de la politique).
    Cancel { name: String },
}

/// `default_path` : chemin proposé par WebView2 (dossier Téléchargements de
/// l'utilisateur + nom déjà dédoublonné). `fallback_dir` sert si ce chemin
/// n'a pas de dossier parent exploitable.
pub fn plan(
    default_path: &Path,
    source_url: &str,
    mime: &str,
    fallback_dir: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Plan {
    let suggested = default_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .or_else(|| url::filename_of(source_url))
        .unwrap_or_default();
    let report = download_policy::analyze(&Request::direct(&suggested, mime, source_url));
    let name = report.normalized_filename;
    if report.verdict == Verdict::Block {
        return Plan::Cancel { name };
    }

    let parent = default_path.parent().filter(|p| !p.as_os_str().is_empty());
    if name == suggested && parent.is_some() {
        return Plan::Save { name, path: None };
    }
    let path = parent.or(fallback_dir).map(|dir| unique_path(dir, &name, &exists));
    Plan::Save { name, path }
}

/// `dir/name`, ou `dir/stem (n).ext` si déjà pris (comme Chromium).
fn unique_path(dir: &Path, name: &str, exists: &impl Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(name);
    if !exists(&first) {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (1..1000)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !exists(p))
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &Path) -> bool {
        false
    }

    #[test]
    fn clean_name_keeps_webview2_path() {
        let p = plan(Path::new("/dl/report.pdf"), "https://example.com/report.pdf", "application/pdf", None, none);
        assert_eq!(p, Plan::Save { name: "report.pdf".into(), path: None });
    }

    #[test]
    fn hostile_name_is_sanitized() {
        // U+202E (RLO) : « invoice<RLO>fdp.exe » s'afficherait « invoiceexe.pdf ».
        let p = plan(Path::new("/dl/invoice\u{202E}fdp.exe"), "https://example.com/x", "application/octet-stream", None, none);
        match p {
            Plan::Save { name, path } => {
                assert_eq!(name, "invoicefdp.exe");
                assert_eq!(path, Some(PathBuf::from("/dl/invoicefdp.exe")));
            }
            other => panic!("{other:?}"),
        }
        // Points finaux retirés (Windows les supprimerait APRÈS classification).
        let p = plan(Path::new("/dl/evil.exe."), "https://example.com/x", "", None, none);
        assert!(matches!(p, Plan::Save { ref name, .. } if name == "evil.exe"));
        // Nom réservé Windows.
        let p = plan(Path::new("/dl/CON.txt"), "https://example.com/x", "text/plain", None, none);
        assert!(matches!(p, Plan::Save { ref name, .. } if name == "_CON.txt"));
    }

    #[test]
    fn dedupes_when_renamed() {
        let taken = |p: &Path| p.ends_with("evil.exe") || p.ends_with("evil (1).exe");
        let p = plan(Path::new("/dl/evil.exe "), "https://example.com/x", "", None, taken);
        assert_eq!(p, Plan::Save { name: "evil.exe".into(), path: Some(PathBuf::from("/dl/evil (2).exe")) });
    }

    #[test]
    fn no_parent_uses_fallback() {
        let p = plan(Path::new(""), "https://example.com/files/a%20b.zip", "application/zip", Some(Path::new("/home/u/Downloads")), none);
        assert_eq!(p, Plan::Save { name: "a b.zip".into(), path: Some(PathBuf::from("/home/u/Downloads/a b.zip")) });
        // Ni parent ni repli : on laisse WebView2 décider.
        let p = plan(Path::new(""), "https://example.com/", "", None, none);
        assert_eq!(p, Plan::Save { name: "download".into(), path: None });
    }

    #[test]
    fn exe_disguised_as_pdf_is_cancelled() {
        let p = plan(Path::new("/dl/facture.pdf"), "https://example.com/facture.pdf", "application/x-msdownload", None, none);
        assert!(matches!(p, Plan::Cancel { .. }), "{p:?}");
    }

    #[test]
    fn unique_path_without_extension() {
        let taken = |p: &Path| p.ends_with("README");
        assert_eq!(unique_path(Path::new("/d"), "README", &taken), PathBuf::from("/d/README (1)"));
        let taken = |p: &Path| p.ends_with(".bashrc");
        assert_eq!(unique_path(Path::new("/d"), ".bashrc", &taken), PathBuf::from("/d/.bashrc (1)"));
    }
}
