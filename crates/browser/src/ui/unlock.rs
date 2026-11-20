//! Coffre au démarrage. Le coffre est toujours chiffré (SQLCipher) ; la clé
//! vient soit d'une clé d'appareil (`vault.key`, défaut, ouverture
//! silencieuse), soit d'un mot de passe principal (optionnel). Toute la
//! logique de mode vit dans `vault::keymode` ; ici, uniquement l'UI GTK.

use std::path::PathBuf;

use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, Dialog, DrawingArea, Entry, InputPurpose, Label, Orientation,
    ResponseType, Window, WindowPosition,
};
use vault::keyfile::OsAccountOnly;
use vault::keymode::{self, KeyStore, Startup};
use vault::{Passphrase, Vault};

/// Paragraphe de bienvenue : présente le mot de passe principal sans l'imposer.
const WELCOME: &str = "Nyx chiffre toujours tes mots de passe, favoris et historique. \
Par défaut, la clé est gardée par ta session : rien à retenir, mais toute personne \
ayant accès à ta session ouverte peut lire les mots de passe enregistrés (comme dans \
Chrome). Un mot de passe principal, optionnel, les protège aussi dans ce cas — mais \
il est irrécupérable si tu l'oublies. Tu pourras le définir plus tard dans les Paramètres.";

/// Fichier de clé présent mais inutilisable : on n'efface rien, on demande.
const DEVICE_KEY_FAILED: &str = "La clé de cet appareil n'ouvre pas le coffre (fichier \
corrompu ou créé par un autre compte). Saisis ton mot de passe principal.";

/// Réponse « Définir un mot de passe principal ».
const DEFINE: ResponseType = ResponseType::Other(1);

/// Emplacement XDG du vault : ~/.local/share/nyx/vault.db
fn vault_path() -> PathBuf {
    let dir = glib::user_data_dir().join("nyx");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("vault.db")
}

/// Coffre + `vault.key` (Linux : protégé par les droits du compte, 0600).
pub fn key_store_for(db: PathBuf) -> KeyStore<OsAccountOnly> {
    KeyStore::new(db, OsAccountOnly)
}

/// Premier lancement = aucun coffre encore créé.
pub fn is_first_launch() -> bool {
    key_store_for(vault_path()).startup() == Startup::FirstLaunch
}

/// Ouvre le coffre au démarrage ; `None` si l'utilisateur quitte.
pub fn unlock_vault() -> Option<Vault> {
    let ks = key_store_for(vault_path());
    match ks.startup() {
        Startup::FirstLaunch => welcome(&ks),
        Startup::DeviceKey => ks.open_with_device_key().ok().or_else(|| ask_master(&ks, Some(DEVICE_KEY_FAILED))),
        Startup::MasterPassword => ask_master(&ks, None),
    }
}

/// Premier lancement : « Plus tard » (clé d'appareil, bouton principal) ou
/// « Définir un mot de passe principal » (révèle les champs).
fn welcome(ks: &KeyStore<OsAccountOnly>) -> Option<Vault> {
    let card = Card::new(None, "Bienvenue dans Nyx", WELCOME);
    let pass = password_entry("mot de passe principal (8 caractères min.)");
    let confirm = password_entry("confirme le mot de passe");
    for e in [&pass, &confirm] {
        e.set_no_show_all(true);
        card.fields.pack_start(e, false, false, 0);
    }
    let define = card.secondary_button("Définir un mot de passe principal", DEFINE);
    let later = card.primary_button("Plus tard", ResponseType::Ok);
    card.dialog.set_default_response(ResponseType::Ok);
    card.dialog.show_all();
    later.grab_focus();

    loop {
        match card.dialog.run() {
            ResponseType::Ok => match ks.create_with_device_key() {
                Ok(v) => return card.close(v),
                Err(e) => card.error(&e.to_string(), None),
            },
            DEFINE if !WidgetExt::is_visible(&pass) => {
                pass.show();
                confirm.show();
                define.set_label("Créer avec ce mot de passe");
                card.dialog.set_default_response(DEFINE);
                pass.grab_focus();
            }
            DEFINE => {
                if let Err(msg) = keymode::check_new_passphrase(&pass.text(), &confirm.text()) {
                    confirm.set_text("");
                    card.error(msg, Some(&pass));
                    continue;
                }
                let secret = take_secret(&pass);
                confirm.set_text("");
                match ks.create_with_master(&secret) {
                    Ok(v) => return card.close(v),
                    Err(e) => card.error(&e.to_string(), Some(&pass)),
                }
            }
            _ => {
                card.dialog.close();
                return None;
            }
        }
    }
}

/// Coffre protégé par un mot de passe principal (ou clé d'appareil
/// inutilisable : `notice` l'explique). Boucle tant que le mot de passe est
/// faux.
fn ask_master(ks: &KeyStore<OsAccountOnly>, notice: Option<&str>) -> Option<Vault> {
    let card = Card::new(
        None,
        "Coffre Nyx verrouillé",
        notice.unwrap_or("Saisis ton mot de passe principal pour déverrouiller mots de passe, favoris et historique."),
    );
    let pass = password_entry("mot de passe principal");
    card.fields.pack_start(&pass, false, false, 4);
    card.secondary_button("Quitter", ResponseType::Cancel);
    card.primary_button("Déverrouiller", ResponseType::Ok);
    card.dialog.set_default_response(ResponseType::Ok);
    card.dialog.show_all();
    pass.grab_focus();

    loop {
        if card.dialog.run() != ResponseType::Ok {
            card.dialog.close();
            return None;
        }
        if pass.text().is_empty() {
            card.error("Le mot de passe est vide", Some(&pass));
            continue;
        }
        match ks.open_with_master(&take_secret(&pass)) {
            Ok(v) => return card.close(v),
            Err(e) => card.error(&e.to_string(), Some(&pass)),
        }
    }
}

/// Lit puis vide le champ (la copie GTK interne échappe à notre contrôle).
pub fn take_secret(entry: &Entry) -> Passphrase {
    let p = Passphrase::new(entry.text().as_str());
    entry.set_text("");
    p
}

/// Carte modale Nyx : lune, titre, texte, champs, erreur, boutons.
pub struct Card {
    pub dialog: Dialog,
    pub fields: GtkBox,
    error: Label,
}

impl Card {
    pub fn new(parent: Option<&Window>, title: &str, text: &str) -> Self {
        let dialog = Dialog::builder()
            .title("Nyx — Coffre")
            .modal(true)
            .resizable(false)
            .default_width(440)
            .window_position(if parent.is_some() { WindowPosition::CenterOnParent } else { WindowPosition::CenterAlways })
            .build();
        if let Some(p) = parent {
            dialog.set_transient_for(Some(p));
        }
        dialog.set_decorated(false); // pose notre propre cadre via le thème
        dialog.style_context().add_class("nyx-confirm");
        dialog.style_context().add_class("nyx-unlock");

        // Croissant de lune Nyx, centré, au-dessus du titre.
        let moon = DrawingArea::new();
        moon.set_size_request(56, 56);
        moon.connect_draw(|a, cr| {
            let (w, h) = (a.allocated_width() as f64, a.allocated_height() as f64);
            draw_moon(cr, w, h);
            gtk::glib::Propagation::Proceed
        });
        moon.set_halign(gtk::Align::Center);

        let title = Label::new(Some(title));
        title.set_halign(gtk::Align::Center);
        title.style_context().add_class("nyx-confirm-title");

        let sub = Label::new(Some(text));
        sub.set_halign(gtk::Align::Center);
        sub.set_line_wrap(true);
        sub.set_max_width_chars(48);
        sub.set_justify(gtk::Justification::Center);
        sub.style_context().add_class("nyx-confirm-body");

        let fields = GtkBox::new(Orientation::Vertical, 8);

        let error = Label::new(None);
        error.set_halign(gtk::Align::Center);
        error.set_line_wrap(true);
        error.set_max_width_chars(48);
        error.style_context().add_class("nyx-error");
        error.set_no_show_all(true);

        let inner = GtkBox::new(Orientation::Vertical, 12);
        inner.set_margin_top(26);
        inner.set_margin_bottom(20);
        inner.set_margin_start(30);
        inner.set_margin_end(30);
        inner.pack_start(&moon, false, false, 0);
        inner.pack_start(&title, false, false, 0);
        inner.pack_start(&sub, false, false, 0);
        inner.pack_start(&fields, false, false, 4);
        inner.pack_start(&error, false, false, 0);
        dialog.content_area().pack_start(&inner, true, true, 0);

        Self { dialog, fields, error }
    }

    fn button(&self, label: &str, response: ResponseType, class: &str) -> Button {
        let b = self.dialog.add_button(label, response).downcast::<Button>().unwrap_or_else(|_| Button::new());
        b.style_context().add_class(class);
        b
    }

    pub fn primary_button(&self, label: &str, response: ResponseType) -> Button {
        self.button(label, response, "nyx-confirm-accept")
    }

    pub fn secondary_button(&self, label: &str, response: ResponseType) -> Button {
        self.button(label, response, "nyx-confirm-cancel")
    }

    /// Affiche `msg` ; vide et focalise `focus` s'il est donné.
    pub fn error(&self, msg: &str, focus: Option<&Entry>) {
        self.error.set_text(msg);
        self.error.show();
        if let Some(e) = focus {
            e.set_text("");
            e.grab_focus();
        }
    }

    pub fn close<T>(&self, value: T) -> Option<T> {
        self.dialog.close();
        Some(value)
    }
}

pub fn password_entry(placeholder: &str) -> Entry {
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
