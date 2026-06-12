use std::path::PathBuf;

use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Dialog, DrawingArea, Entry, InputPurpose, Label, Orientation,
    ResponseType, WindowPosition,
};
use vault::Vault;


/// Emplacement XDG du vault : ~/.local/share/nyx/vault.db
fn vault_path() -> PathBuf {
    let dir = glib::user_data_dir().join("nyx");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("vault.db")
}

/// Dialogue de déverrouillage au démarrage (Sprint 2.1).
/// Boucle tant que la passphrase est mauvaise ; None si l'utilisateur quitte.
/// Au premier lancement, le vault est créé (passphrase + confirmation).
pub fn unlock_vault() -> Option<Vault> {
    let path = vault_path();
    let creating = !path.exists();

    let dialog = Dialog::builder()
        .title("Nyx — Vault")
        .modal(true)
        .resizable(false)
        .default_width(400)
        .window_position(WindowPosition::CenterAlways)
        .build();
    dialog.set_decorated(false); // pose notre propre cadre via le thème
    dialog.style_context().add_class("nyx-unlock");
    dialog.add_button("Quitter", ResponseType::Cancel);
    dialog.add_button(
        if creating { "Créer le vault" } else { "Déverrouiller" },
        ResponseType::Ok,
    );
    dialog.set_default_response(ResponseType::Ok);

    // Croissant de lune Nyx, centré, au-dessus du titre.
    let moon = DrawingArea::new();
    moon.set_size_request(56, 56);
    moon.connect_draw(|a, cr| {
        let (w, h) = (a.allocated_width() as f64, a.allocated_height() as f64);
        draw_moon(cr, w, h);
        gtk::glib::Propagation::Proceed
    });
    moon.set_halign(gtk::Align::Center);

    let title = Label::new(Some(if creating {
        "Crée la passphrase de ton coffre Nyx"
    } else {
        "Coffre Nyx verrouillé"
    }));
    title.set_halign(gtk::Align::Center);
    title.style_context().add_class("nyx-unlock-title");

    let subtitle = Label::new(Some(if creating {
        "Elle chiffre tes mots de passe, favoris et historique. Irrécupérable si oubliée."
    } else {
        "Saisis ta passphrase pour déverrouiller mots de passe, favoris et historique."
    }));
    subtitle.set_halign(gtk::Align::Center);
    subtitle.set_line_wrap(true);
    subtitle.set_max_width_chars(38);
    subtitle.set_justify(gtk::Justification::Center);
    subtitle.style_context().add_class("nyx-unlock-sub");

    let passphrase = password_entry("passphrase");
    let confirm = password_entry("confirme la passphrase");

    let error = Label::new(None);
    error.set_halign(gtk::Align::Center);
    error.style_context().add_class("nyx-error");
    error.set_no_show_all(true);

    let inner = GtkBox::new(Orientation::Vertical, 12);
    inner.set_margin_top(26);
    inner.set_margin_bottom(20);
    inner.set_margin_start(30);
    inner.set_margin_end(30);
    inner.pack_start(&moon, false, false, 0);
    inner.pack_start(&title, false, false, 0);
    inner.pack_start(&subtitle, false, false, 0);
    inner.pack_start(&passphrase, false, false, 4);
    if creating {
        inner.pack_start(&confirm, false, false, 0);
    }
    inner.pack_start(&error, false, false, 0);

    dialog.content_area().pack_start(&inner, true, true, 0);

    dialog.show_all();
    passphrase.grab_focus();

    let show_error = |msg: &str| {
        error.set_text(msg);
        error.show();
        passphrase.select_region(0, -1);
        passphrase.grab_focus();
    };

    loop {
        if dialog.run() != ResponseType::Ok {
            dialog.close();
            return None;
        }

        let pass = passphrase.text();
        if pass.is_empty() {
            show_error("La passphrase est vide");
            continue;
        }
        if creating && pass != confirm.text() {
            show_error("Les passphrases ne correspondent pas");
            continue;
        }

        match Vault::open(&path, &pass) {
            Ok(vault) => {
                dialog.close();
                return Some(vault);
            }
            Err(_) => show_error("Passphrase invalide"),
        }
    }
}

fn password_entry(placeholder: &str) -> Entry {
    let entry = Entry::builder()
        .visibility(false)
        .input_purpose(InputPurpose::Password)
        .activates_default(true)
        .placeholder_text(placeholder)
        .build();
    entry.style_context().add_class("nyx-urlbar");
    entry
}

/// Croissant de lune Nyx (#9d6ef7) — primitive Cairo locale, sans dépendre
/// d'un module d'icônes : disque plein évidé par un disque décalé.
fn draw_moon(cr: &gtk::cairo::Context, w: f64, h: f64) {
    let s = w.min(h) / 24.0;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let p = |x: f64, y: f64| ((x - 12.0) * s + cx, (y - 12.0) * s + cy);
    cr.set_source_rgba(0.612, 0.431, 0.969, 0.95);
    let (px, py) = p(13.0, 12.0);
    cr.arc(px, py, 7.5 * s, 1.9, 4.6);
    let (qx, qy) = p(15.5, 12.0);
    cr.arc_negative(qx, qy, 7.0 * s, 4.6, 1.9);
    cr.close_path();
    let _ = cr.fill();
}
