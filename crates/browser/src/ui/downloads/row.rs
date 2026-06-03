//! Une ligne de téléchargement dans la liste.
//!
//! Layout :
//!   ```
//!   ┌─────────────────────────────────────────────────────┐
//!   │ filename.ext                          ↗  📂  ×       │
//!   │ source.tld  ·  status / progress                     │
//!   └─────────────────────────────────────────────────────┘
//!   ```
//!
//! Actions :
//!   - ↗ : `actions::run` (ouvrir / exécuter selon type ; re-check si activé)
//!   - 📂 : `actions::reveal` (file manager avec sélection si possible)
//!   - × : supprime de la liste (jamais du disque)
//!
//! Les actions d'I/O sont désactivées tant que le download est `InProgress`
//! ou `Cancelled` / `Failed` (le fichier n'est pas un livrable utilisable).

use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Label, Orientation, ProgressBar, Window};

use nyx_core::downloads::{DownloadEntry, DownloadStatus};

use crate::state::downloads::DownloadsHandle;
use crate::state::settings::Settings;

use super::actions;
use super::popover::Refresh;

/// Construit la ligne complète. `on_changed` est appelé après suppression
/// pour redéclencher le re-render du popover.
pub fn build(
    entry: &DownloadEntry,
    handle: DownloadsHandle,
    settings: Settings,
    parent: Option<Window>,
    on_changed: Refresh,
) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.style_context().add_class("nyx-dl-row");

    // ─── Colonne info (gauche) ─────
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

    // ─── Progress bar (pour InProgress) ─────
    if let DownloadStatus::InProgress { .. } = &entry.status {
        let bar = ProgressBar::new();
        bar.style_context().add_class("nyx-dl-bar");
        match entry.status.fraction() {
            Some(f) => bar.set_fraction(f),
            None => bar.pulse(),
        }
        info.pack_start(&bar, false, false, 2);
    }

    row.pack_start(&info, true, true, 0);

    // ─── Colonne actions (droite) ─────
    let actions_box = GtkBox::new(Orientation::Horizontal, 2);

    let runable = matches!(entry.status, DownloadStatus::Completed);
    if runable {
        let run_btn = action_btn("↗", "Ouvrir");
        let rev_btn = action_btn("📂", "Afficher dans le dossier");

        {
            let (e, s, p) = (entry.clone(), settings.clone(), parent.clone());
            run_btn.connect_clicked(move |_| actions::run(&e, &s, p.as_ref()));
        }
        {
            let e = entry.clone();
            rev_btn.connect_clicked(move |_| actions::reveal(&e));
        }

        actions_box.pack_start(&run_btn, false, false, 0);
        actions_box.pack_start(&rev_btn, false, false, 0);
    }

    let del_btn = action_btn("×", "Retirer de la liste");
    {
        let (h, id, cb) = (handle, entry.id, on_changed);
        del_btn.connect_clicked(move |_| {
            h.borrow_mut().remove(id);
            cb();
        });
    }
    actions_box.pack_start(&del_btn, false, false, 0);

    row.pack_end(&actions_box, false, false, 0);

    row
}

fn subline(entry: &DownloadEntry) -> String {
    let origin = display_origin(&entry.source_origin);
    let status = display_status(&entry.status);
    if origin.is_empty() { status }
    else if status.is_empty() { origin }
    else { format!("{origin}  ·  {status}") }
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

fn action_btn(label: &str, tooltip: &str) -> Button {
    let btn = Button::with_label(label);
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));
    btn.style_context().add_class("nyx-nav-btn");
    btn
}
