//! Téléchargements (Sprint 4.3) : destination automatique dans
//! ~/Téléchargements (nom assaini + dédoublonné), barre d'état discrète.
//!
//! Chaque onglet a son propre `WebContext` (isolation / mode privé) :
//! `wire_context` doit donc être appelé pour chaque contexte créé — la barre
//! d'état, elle, est unique (enregistrée par `init`, partagée en thread_local).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{Box as GtkBox, Label, Revealer, RevealerTransitionType};
use webkit2gtk::{DownloadExt, WebContext, WebContextExt};

thread_local! {
    static BAR: RefCell<Option<(Revealer, Label)>> = const { RefCell::new(None) };
}

/// Crée la barre d'état et l'ajoute en bas du conteneur. À appeler une fois.
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

    BAR.with(|b| *b.borrow_mut() = Some((revealer, label)));
}

/// Branche la gestion des téléchargements sur un WebContext.
pub fn wire_context(ctx: &WebContext) {
    ctx.connect_download_started(|_, download| {
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

        show("Téléchargement…");

        download.connect_estimated_progress_notify(|d| {
            let percent = (d.estimated_progress() * 100.0) as u32;
            show(&format!("⬇ {} — {percent}%", destination_name(d)));
        });
        download.connect_finished(|d| {
            show(&format!("✓ {} téléchargé", destination_name(d)));
            hide_later();
        });
        download.connect_failed(|d, error| {
            show(&format!("✗ {} : {error}", destination_name(d)));
            hide_later();
        });
    });
}

fn show(message: &str) {
    BAR.with(|b| {
        if let Some((revealer, label)) = b.borrow().as_ref() {
            label.set_text(message);
            revealer.set_reveal_child(true);
        }
    });
}

fn hide_later() {
    glib::timeout_add_local_once(Duration::from_secs(5), || {
        BAR.with(|b| {
            if let Some((revealer, _)) = b.borrow().as_ref() {
                revealer.set_reveal_child(false);
            }
        });
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
