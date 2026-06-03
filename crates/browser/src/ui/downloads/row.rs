//! Une ligne de téléchargement dans la liste.
//!
//! Layout :
//!   ```
//!   ┌─────────────────────────────────────────────────────┐
//!   │ filename.ext                          ▶  📂  ×       │
//!   │ source.tld  ·  status / progress                     │
//!   └─────────────────────────────────────────────────────┘
//!   ```
//!
//! Actions (icônes GTK symboliques pour rendu fiable + thème-aware) :
//!   - ▶ `media-playback-start-symbolic` → `actions::run`
//!   - 📂 `document-open-symbolic`        → `actions::reveal`
//!   - × `window-close-symbolic`          → retire de la liste
//!
//! Les boutons ne s'affichent QUE pour les downloads `Completed` ; un
//! `InProgress` / `Cancelled` / `Failed` n'a que le bouton « retirer ».

use gtk::prelude::*;
use gtk::{Box as GtkBox, Label, Orientation, ProgressBar, Window};

use nyx_core::downloads::{DownloadEntry, DownloadStatus};

use crate::state::downloads::DownloadsHandle;
use crate::state::settings::Settings;

use super::popover::Refresh;
use super::{actions, widgets};

/// Construit la ligne complète.
pub fn build(
    entry: &DownloadEntry,
    handle: DownloadsHandle,
    settings: Settings,
    parent: Option<Window>,
    on_changed: Refresh,
) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.style_context().add_class("nyx-dl-row");

    row.pack_start(&build_info(entry), true, true, 0);
    row.pack_end(&build_actions(entry, handle, settings, parent, on_changed),
                 false, false, 0);
    row
}

fn build_info(entry: &DownloadEntry) -> GtkBox {
    let info = GtkBox::new(Orientation::Vertical, 2);

    let name = Label::new(Some(&truncate(&entry.filename, 42)));
    name.set_xalign(0.0);
    name.style_context().add_class("nyx-dl-name");
    name.set_tooltip_text(Some(&entry.filename));

    let sub = Label::new(Some(&subline(entry)));
    sub.set_xalign(0.0);
    sub.style_context().add_class("nyx-dl-sub");

    info.pack_start(&name, false, false, 0);
    info.pack_start(&sub, false, false, 0);

    if let DownloadStatus::InProgress { .. } = &entry.status {
        let bar = ProgressBar::new();
        bar.style_context().add_class("nyx-dl-bar");
        match entry.status.fraction() {
            Some(f) => bar.set_fraction(f),
            None => bar.pulse(),
        }
        info.pack_start(&bar, false, false, 2);
    }
    info
}

fn build_actions(
    entry: &DownloadEntry,
    handle: DownloadsHandle,
    settings: Settings,
    parent: Option<Window>,
    on_changed: Refresh,
) -> GtkBox {
    let actions_box = GtkBox::new(Orientation::Horizontal, 2);
    let completed = matches!(entry.status, DownloadStatus::Completed);

    if completed {
        let run_btn = widgets::icon_only("media-playback-start-symbolic", "Ouvrir");
        let rev_btn = widgets::icon_only("document-open-symbolic",        "Afficher");

        {
            let (e, s, p) = (entry.clone(), settings, parent);
            run_btn.connect_clicked(move |_| actions::run(&e, &s, p.as_ref()));
        }
        {
            let e = entry.clone();
            rev_btn.connect_clicked(move |_| actions::reveal(&e));
        }
        actions_box.pack_start(&run_btn, false, false, 0);
        actions_box.pack_start(&rev_btn, false, false, 0);
    }

    let del_btn = widgets::icon_only("window-close-symbolic", "Retirer");
    {
        let (h, id, cb) = (handle, entry.id, on_changed);
        del_btn.connect_clicked(move |_| {
            h.borrow_mut().remove(id);
            cb();
        });
    }
    actions_box.pack_start(&del_btn, false, false, 0);
    actions_box
}

fn subline(entry: &DownloadEntry) -> String {
    let origin = display_origin(&entry.source_origin);
    let status = display_status(&entry.status);
    match (origin.is_empty(), status.is_empty()) {
        (true, true)   => String::new(),
        (true, false)  => status,
        (false, true)  => origin,
        (false, false) => format!("{origin}  ·  {status}"),
    }
}

fn display_origin(o: &str) -> String {
    let s = o.strip_prefix("https://").unwrap_or(o);
    let s = s.strip_prefix("http://").unwrap_or(s);
    s.to_string()
}

fn display_status(s: &DownloadStatus) -> String {
    match s {
        DownloadStatus::InProgress { received, total: Some(t) } =>
            format!("{} / {}", human_size(*received), human_size(*t)),
        DownloadStatus::InProgress { received, total: None } =>
            human_size(*received),
        DownloadStatus::Completed => "Terminé".into(),
        DownloadStatus::Cancelled => "Annulé".into(),
        DownloadStatus::Failed { reason } => format!("Échec : {reason}"),
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["o", "Ko", "Mo", "Go", "To"];
    let (mut b, mut i) = (bytes as f64, 0);
    while b >= 1024.0 && i < UNITS.len() - 1 { b /= 1024.0; i += 1; }
    if i == 0 { format!("{} {}", bytes, UNITS[0]) }
    else      { format!("{b:.1} {}", UNITS[i]) }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
