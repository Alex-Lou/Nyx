// Sprint 4.3 — téléchargements : destination automatique dans ~/Téléchargements
// (nom dédoublonné), barre d'état discrète en bas de fenêtre.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{Box as GtkBox, Label, Revealer, RevealerTransitionType};
use webkit2gtk::{DownloadExt, WebContext, WebContextExt};

/// Branche la gestion des téléchargements et ajoute la barre d'état
/// au bas du conteneur donné.
pub fn init(container: &GtkBox) {
    let label = Label::new(None);
    label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    label.set_xalign(0.0);

    let bar = GtkBox::new(gtk::Orientation::Horizontal, 8);
    bar.style_context().add_class("nyx-downloadbar");
    bar.pack_start(&label, true, true, 12);

    let revealer = Revealer::builder()
        .transition_type(RevealerTransitionType::SlideUp)
        .transition_duration(180)
        .reveal_child(false)
        .build();
    revealer.add(&bar);
    container.pack_end(&revealer, false, false, 0);

    let Some(ctx) = WebContext::default() else { return };
    ctx.connect_download_started(move |_, download| {
        download.connect_decide_destination(|d, suggested| {
            let name = sanitize(suggested);
            let dir = glib::user_special_dir(glib::UserDirectory::Downloads)
                .unwrap_or_else(glib::home_dir);
            let path = unique_path(&dir, &name);
            match glib::filename_to_uri(&path, None) {
                Ok(uri) => {
                    d.set_destination(&uri);
                    true
                }
                Err(_) => false,
            }
        });

        let rev = revealer.clone();
        let lbl = label.clone();
        rev.set_reveal_child(true);
        lbl.set_text("Téléchargement…");

        let lbl2 = lbl.clone();
        download.connect_estimated_progress_notify(move |d| {
            let name = destination_name(d);
            let percent = (d.estimated_progress() * 100.0) as u32;
            lbl2.set_text(&format!("⬇ {name} — {percent}%"));
        });

        let rev2 = rev.clone();
        let lbl2 = lbl.clone();
        download.connect_finished(move |d| {
            lbl2.set_text(&format!("✓ {} téléchargé", destination_name(d)));
            hide_later(&rev2);
        });

        let rev2 = rev.clone();
        let lbl2 = lbl.clone();
        download.connect_failed(move |d, error| {
            lbl2.set_text(&format!("✗ {} : {error}", destination_name(d)));
            hide_later(&rev2);
        });
    });
}

fn hide_later(revealer: &Revealer) {
    let rev = revealer.clone();
    glib::timeout_add_local_once(Duration::from_secs(5), move || {
        rev.set_reveal_child(false);
    });
}

fn destination_name(download: &webkit2gtk::Download) -> String {
    download
        .destination()
        .and_then(|uri| glib::filename_from_uri(&uri).ok())
        .and_then(|(path, _)| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| String::from("fichier"))
}

/// Nom de fichier sûr : pas de séparateurs, pas de fichier caché, pas vide.
fn sanitize(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | '\0') { '_' } else { c })
        .collect();
    let clean = clean.trim_start_matches('.').trim();
    if clean.is_empty() { String::from("téléchargement") } else { clean.to_string() }
}

/// Évite d'écraser un fichier existant : "doc.pdf" → "doc (1).pdf", …
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, Some(e)),
        _ => (name, None),
    };
    for n in 1.. {
        let next = match ext {
            Some(e) => dir.join(format!("{stem} ({n}).{e}")),
            None => dir.join(format!("{stem} ({n})")),
        };
        if !next.exists() {
            return next;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nom_de_fichier_assaini() {
        assert_eq!(sanitize("doc.pdf"), "doc.pdf");
        assert_eq!(sanitize("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize(""), "téléchargement");
        assert_eq!(sanitize(".bashrc"), "bashrc");
    }

    #[test]
    fn chemin_dedoublonne() {
        let dir = std::env::temp_dir().join(format!("nyx_dl_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a.txt"));
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a (1).txt"));
        std::fs::write(dir.join("a (1).txt"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a (2).txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
