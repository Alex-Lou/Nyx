//! Assistant « Importer depuis Chrome » : dialog GTK (profil, cases, fichiers
//! optionnels) puis résumé. Les fichiers sont lus au clic sur « Importer »,
//! l'import est assez rapide pour rester sur le thread principal.
//!
//! Déclenché uniquement par `nyx://import/chrome` depuis une page interne
//! (cf. `security::Verdict::ImportChrome`).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{
    Box as GtkBox, CheckButton, Dialog, DialogFlags, FileChooserAction, FileChooserButton, Label,
    MessageDialog, MessageType, Orientation, RadioButton, ResponseType, Window,
};
use nyx_core::import::chrome::{self, ChromeProfile};
use nyx_core::import::passwords;
use vault::Vault;

use crate::state::bookmarks::{self, Bookmarks};
use crate::state::vault_sync;

/// Plafond d'historique lu (aligné sur le plafond du vault).
const HISTORY_LIMIT: usize = 5000;

struct Options {
    profile: Option<ChromeProfile>,
    bookmarks: bool,
    history: bool,
    csv: Option<PathBuf>,
    html: Option<PathBuf>,
}

#[derive(Default)]
struct Summary {
    bookmarks: usize,
    history: usize,
    passwords: usize,
    ignored: usize,
    errors: Vec<String>,
}

impl Summary {
    fn text(&self) -> String {
        let mut s = format!(
            "{} favoris, {} pages d'historique, {} mots de passe importés ({} ignorés)",
            fmt_n(self.bookmarks), fmt_n(self.history), fmt_n(self.passwords), fmt_n(self.ignored),
        );
        for e in &self.errors {
            s.push_str(&format!("\n\n⚠ {e}"));
        }
        s
    }
}

/// 4870 → "4 870".
fn fmt_n(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn detect_profiles() -> Vec<ChromeProfile> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    chrome::find_profiles(&chrome::candidate_bases(home.as_deref(), Path::new("/mnt/c/Users")))
}

/// Ouvre le dialog d'import (non bloquant).
pub fn run(parent: Option<&Window>, bm: &Bookmarks, vault: &Rc<Vault>) {
    let profiles = detect_profiles();
    let dlg = Dialog::with_buttons(
        Some("Importer depuis Chrome"),
        parent,
        DialogFlags::MODAL | DialogFlags::DESTROY_WITH_PARENT,
        &[("Annuler", ResponseType::Cancel), ("Importer", ResponseType::Accept)],
    );
    dlg.set_default_size(520, -1);
    let area = dlg.content_area();
    area.set_spacing(8);
    area.set_margin_start(16);
    area.set_margin_end(16);
    area.set_margin_top(12);

    let hint = Label::new(Some("Fermez Chrome avant d'importer l'historique (fichier verrouillé)."));
    hint.set_xalign(0.0);
    area.pack_start(&hint, false, false, 0);

    // Profils détectés (radio). Aucun → on le dit, les fichiers restent utilisables.
    let mut radios: Vec<RadioButton> = Vec::new();
    for p in &profiles {
        let r = match radios.first() {
            Some(first) => RadioButton::with_label_from_widget(first, &p.label),
            None => RadioButton::with_label(&p.label),
        };
        area.pack_start(&r, false, false, 0);
        radios.push(r);
    }
    if profiles.is_empty() {
        let none = Label::new(Some("Aucun profil Chrome détecté."));
        none.set_xalign(0.0);
        area.pack_start(&none, false, false, 0);
    }

    let cb_bm = CheckButton::with_label("Favoris");
    let cb_hist = CheckButton::with_label("Historique");
    cb_bm.set_active(true);
    cb_hist.set_active(true);
    area.pack_start(&cb_bm, false, false, 0);
    area.pack_start(&cb_hist, false, false, 0);

    let csv_btn = file_row(&area, "Fichier CSV de mots de passe…");
    let html_btn = file_row(&area, "Fichier HTML de favoris…");
    area.show_all();

    let (bm, vault) = (bm.clone(), vault.clone());
    let parent = parent.cloned();
    dlg.connect_response(move |dlg, resp| {
        if resp == ResponseType::Accept {
            let opts = Options {
                profile: radios.iter().position(|r| r.is_active()).map(|i| profiles[i].clone()),
                bookmarks: cb_bm.is_active(),
                history: cb_hist.is_active(),
                csv: csv_btn.filename(),
                html: html_btn.filename(),
            };
            let summary = do_import(&opts, &bm, &vault);
            show_summary(parent.as_ref(), &summary);
        }
        dlg.close();
    });
    dlg.show();
}

fn file_row(area: &GtkBox, title: &str) -> FileChooserButton {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    let label = Label::new(Some(title));
    label.set_xalign(0.0);
    let btn = FileChooserButton::new(title, FileChooserAction::Open);
    btn.set_hexpand(true);
    row.pack_start(&label, false, false, 0);
    row.pack_start(&btn, true, true, 0);
    area.pack_start(&row, false, false, 0);
    btn
}

fn show_summary(parent: Option<&Window>, s: &Summary) {
    let kind = if s.errors.is_empty() { MessageType::Info } else { MessageType::Warning };
    let m = MessageDialog::new(parent, DialogFlags::MODAL, kind, gtk::ButtonsType::Ok, &s.text());
    m.set_title("Import terminé");
    m.connect_response(|d, _| d.close());
    m.show();
}

/// Lit les fichiers puis écrit dans le store/vault. Pas de GTK ici.
fn do_import(o: &Options, bm: &Bookmarks, vault: &Rc<Vault>) -> Summary {
    let mut s = Summary::default();

    // Favoris : profil Chrome (JSON) + export HTML éventuel, dédup par URL.
    let mut found = Vec::new();
    if let (true, Some(p)) = (o.bookmarks, &o.profile) {
        match std::fs::read_to_string(p.dir.join("Bookmarks")) {
            Ok(json) => found.extend(chrome::parse_bookmarks_json(&json)),
            Err(e) => s.errors.push(format!("Favoris : {e}")),
        }
    }
    if let Some(path) = &o.html {
        match std::fs::read_to_string(path) {
            Ok(html) => found.extend(bookmarks::import_netscape(&html)),
            Err(e) => s.errors.push(format!("Fichier HTML : {e}")),
        }
    }
    for b in found {
        if bookmarks::add_in(bm, b.url, b.title, b.folder) {
            s.bookmarks += 1;
        }
    }
    if s.bookmarks > 0 {
        vault_sync::persist_bookmarks(bm, vault);
    }

    // Historique : copie + lecture SQLite, puis insertion datée.
    if let (true, Some(p)) = (o.history, &o.profile) {
        match vault::read_chrome_history(&p.dir.join("History"), HISTORY_LIMIT) {
            Ok(rows) => {
                let entries: Vec<(String, String, i64)> = rows
                    .into_iter()
                    .filter(|(url, _, _)| nyx_core::url::is_navigable(url))
                    .filter_map(|(url, title, t)| Some((url, title, chrome::chrome_time_to_unix(t)?)))
                    .collect();
                match vault.import_history(&entries) {
                    Ok(n) => s.history = n,
                    Err(e) => s.errors.push(format!("Historique : {e}")),
                }
            }
            Err(e) => s.errors.push(format!("Historique : {e:#}")),
        }
    }

    // Mots de passe : CSV exporté (jamais de déchiffrement Chrome).
    if let Some(path) = &o.csv {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let parsed = passwords::parse(&text);
                s.ignored += parsed.skipped;
                for l in parsed.logins {
                    let existing = vault.passwords_for(&l.host).unwrap_or_default();
                    // Même hôte + identifiant : on garde l'existant (choix sûr).
                    if existing.iter().any(|p| p.username == l.username) {
                        s.ignored += 1;
                    } else if vault.add_password(&vault::Password::new(l.host, l.username, l.password)).is_ok() {
                        s.passwords += 1;
                    } else {
                        s.ignored += 1;
                    }
                }
            }
            Err(e) => s.errors.push(format!("Fichier CSV : {e}")),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::fmt_n;

    #[test]
    fn thousands() {
        assert_eq!(fmt_n(0), "0");
        assert_eq!(fmt_n(312), "312");
        assert_eq!(fmt_n(4870), "4 870");
        assert_eq!(fmt_n(1234567), "1 234 567");
    }
}
