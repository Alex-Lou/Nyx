use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, Notebook, Orientation, Revealer, RevealerTransitionType,
    SearchEntry,
};
use webkit2gtk::{FindController, FindControllerExt, FindOptions, WebViewExt};

use crate::tabs::current_webview;

/// Barre de recherche dans la page (Sprint 4.2, Ctrl+F).
#[derive(Clone)]
pub struct FindBar {
    pub widget: Revealer,
    entry: SearchEntry,
    notebook: Notebook,
}

impl FindBar {
    pub fn new(notebook: &Notebook) -> Self {
        let entry = SearchEntry::new();
        entry.set_placeholder_text(Some("Rechercher dans la page…"));
        entry.set_width_chars(32);

        let prev_btn  = small_button("▲", "Précédent");
        let next_btn  = small_button("▼", "Suivant");
        let close_btn = small_button("×", "Fermer (Échap)");

        let bar = GtkBox::new(Orientation::Horizontal, 4);
        bar.style_context().add_class("nyx-findbar");
        bar.pack_start(&entry, false, false, 8);
        bar.pack_start(&prev_btn, false, false, 0);
        bar.pack_start(&next_btn, false, false, 0);
        bar.pack_start(&close_btn, false, false, 4);

        let widget = Revealer::builder()
            .transition_type(RevealerTransitionType::SlideDown)
            .transition_duration(180)
            .reveal_child(false)
            .build();
        widget.add(&bar);

        let findbar = Self {
            widget,
            entry,
            notebook: notebook.clone(),
        };
        findbar.wire(&prev_btn, &next_btn, &close_btn);
        findbar
    }

    fn wire(&self, prev_btn: &Button, next_btn: &Button, close_btn: &Button) {
        let fb = self.clone();
        self.entry.connect_search_changed(move |entry| {
            let text = entry.text();
            let Some(fc) = fb.controller() else { return };
            if text.is_empty() {
                fc.search_finish();
            } else {
                let options = FindOptions::CASE_INSENSITIVE | FindOptions::WRAP_AROUND;
                fc.search(&text, options.bits(), 500);
            }
        });

        // Entrée → occurrence suivante
        let fb = self.clone();
        self.entry.connect_activate(move |_| {
            if let Some(fc) = fb.controller() { fc.search_next(); }
        });

        // Échap dans le champ → fermer (signal stop-search de SearchEntry)
        let fb = self.clone();
        self.entry.connect_stop_search(move |_| fb.close());

        let fb = self.clone();
        prev_btn.connect_clicked(move |_| {
            if let Some(fc) = fb.controller() { fc.search_previous(); }
        });
        let fb = self.clone();
        next_btn.connect_clicked(move |_| {
            if let Some(fc) = fb.controller() { fc.search_next(); }
        });
        let fb = self.clone();
        close_btn.connect_clicked(move |_| fb.close());
    }

    pub fn open(&self) {
        self.widget.set_reveal_child(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    pub fn close(&self) {
        if let Some(fc) = self.controller() {
            fc.search_finish();
        }
        self.widget.set_reveal_child(false);
        if let Some(wv) = current_webview(&self.notebook) {
            wv.grab_focus();
        }
    }

    fn controller(&self) -> Option<FindController> {
        current_webview(&self.notebook)?.find_controller()
    }
}

fn small_button(label: &str, tooltip: &str) -> Button {
    let btn = Button::with_label(label);
    btn.style_context().add_class("nyx-nav-btn");
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));
    btn
}
