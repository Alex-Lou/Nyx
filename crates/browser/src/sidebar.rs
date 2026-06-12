use std::rc::Rc;

use gtk::pango::EllipsizeMode;
use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation, PolicyType,
    Revealer, RevealerTransitionType, ScrolledWindow, SearchEntry, SelectionMode,
    Stack, StackSwitcher,
};
use vault::Vault;
use webkit2gtk::WebViewExt;

use crate::tabs::TabBar;

const SIDEBAR_WIDTH: i32 = 320;
const HISTORY_LIMIT: usize = 100;

/// Sidebar rétractable : Historique / Favoris / Mots de passe
/// (Sprint 2.3, 2.5, 2.7).
#[derive(Clone)]
pub struct Sidebar {
    pub widget: Revealer,
    vault: Rc<Vault>,
    tabs: TabBar,
    history_list: ListBox,
    history_search: SearchEntry,
    bookmarks_list: ListBox,
    passwords_list: ListBox,
}

impl Sidebar {
    pub fn new(vault: Rc<Vault>, tabs: TabBar) -> Self {
        let history_list = new_list();
        let bookmarks_list = new_list();
        let passwords_list = new_list();

        let history_search = SearchEntry::new();
        history_search.set_placeholder_text(Some("Rechercher…"));

        let clear_btn = Button::with_label("Effacer l'historique");
        clear_btn.style_context().add_class("nyx-danger-btn");
        clear_btn.set_relief(gtk::ReliefStyle::None);

        let history_panel = GtkBox::new(Orientation::Vertical, 6);
        history_panel.pack_start(&history_search, false, false, 0);
        history_panel.pack_start(&scrolled(&history_list), true, true, 0);
        history_panel.pack_start(&clear_btn, false, false, 0);

        let stack = Stack::new();
        stack.add_titled(&history_panel, "history", "Historique");
        stack.add_titled(&scrolled(&bookmarks_list), "bookmarks", "Favoris");
        stack.add_titled(&scrolled(&passwords_list), "passwords", "Clés");

        let switcher = StackSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_halign(gtk::Align::Center);

        let root = GtkBox::new(Orientation::Vertical, 8);
        root.style_context().add_class("nyx-sidebar");
        root.set_width_request(SIDEBAR_WIDTH);
        root.pack_start(&switcher, false, false, 8);
        root.pack_start(&stack, true, true, 0);

        let widget = Revealer::builder()
            .transition_type(RevealerTransitionType::SlideRight)
            .transition_duration(220)
            .reveal_child(false)
            .build();
        widget.add(&root);

        let sidebar = Self {
            widget,
            vault,
            tabs,
            history_list,
            history_search,
            bookmarks_list,
            passwords_list,
        };
        sidebar.wire(&clear_btn);
        sidebar
    }

    fn wire(&self, clear_btn: &Button) {
        // Recherche historique (Sprint 2.3)
        let sb = self.clone();
        self.history_search
            .connect_search_changed(move |_| sb.refresh_history());

        // Clic sur une entrée → charger dans l'onglet actif
        for list in [&self.history_list, &self.bookmarks_list] {
            let tabs = self.tabs.clone();
            list.connect_row_activated(move |_, row| {
                let url = row.widget_name();
                if url.is_empty() {
                    return;
                }
                match tabs.active_webview() {
                    Some(wv) => wv.load_uri(&url),
                    None => {
                        tabs.open(&url);
                    }
                }
            });
        }

        let sb = self.clone();
        clear_btn.connect_clicked(move |_| {
            if sb.vault.clear_history().is_ok() {
                sb.refresh_history();
            }
        });
    }

    /// Ouvre/ferme la sidebar ; rafraîchit les panels à l'ouverture.
    pub fn toggle(&self) {
        let show = !self.widget.reveals_child();
        if show {
            self.refresh_all();
        }
        self.widget.set_reveal_child(show);
    }

    pub fn refresh_all(&self) {
        self.refresh_history();
        self.refresh_bookmarks();
        self.refresh_passwords();
    }

    fn refresh_history(&self) {
        clear_list(&self.history_list);
        let query = self.history_search.text();
        let entries = if query.is_empty() {
            self.vault.recent_history(HISTORY_LIMIT)
        } else {
            self.vault.search_history(&query)
        }
        .unwrap_or_default();

        for e in entries {
            let title = if e.title.is_empty() { e.url.clone() } else { e.title.clone() };
            self.history_list.add(&link_row(&title, &e.url, &e.url, vec![]));
        }
        self.history_list.show_all();
    }

    pub fn refresh_bookmarks(&self) {
        clear_list(&self.bookmarks_list);
        for b in self.vault.list_bookmarks().unwrap_or_default() {
            let subtitle = if b.tags.is_empty() {
                b.url.clone()
            } else {
                format!("{}   #{}", b.url, b.tags.join(" #"))
            };

            let delete = small_button("×");
            let sb = self.clone();
            let id = b.id.unwrap_or_default();
            delete.connect_clicked(move |_| {
                if sb.vault.delete_bookmark(id).is_ok() {
                    sb.refresh_bookmarks();
                }
            });

            self.bookmarks_list
                .add(&link_row(&b.title, &subtitle, &b.url, vec![delete]));
        }
        self.bookmarks_list.show_all();
    }

    fn refresh_passwords(&self) {
        clear_list(&self.passwords_list);
        for domain in self.vault.list_password_domains().unwrap_or_default() {
            for p in self.vault.passwords_for(&domain).unwrap_or_default() {
                let copy = small_button("⧉");
                copy.set_tooltip_text(Some("Copier le mot de passe"));
                let secret = p.password.clone();
                copy.connect_clicked(move |_| {
                    gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD).set_text(&secret);
                });

                let delete = small_button("×");
                let sb = self.clone();
                let id = p.id.unwrap_or_default();
                delete.connect_clicked(move |_| {
                    if sb.vault.delete_password(id).is_ok() {
                        sb.refresh_passwords();
                    }
                });

                self.passwords_list
                    .add(&link_row(&p.username, &domain, "", vec![copy, delete]));
            }
        }
        self.passwords_list.show_all();
    }
}

fn new_list() -> ListBox {
    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::None);
    list
}

fn scrolled(child: &ListBox) -> ScrolledWindow {
    let sw = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .build();
    sw.add(child);
    sw
}

fn clear_list(list: &ListBox) {
    for child in list.children() {
        list.remove(&child);
    }
}

fn small_button(label: &str) -> Button {
    let btn = Button::with_label(label);
    btn.set_relief(gtk::ReliefStyle::None);
    btn.style_context().add_class("nyx-row-btn");
    btn
}

/// Ligne titre + sous-titre, avec boutons d'action optionnels.
/// `url` non vide → la ligne est activable et porte l'URL.
fn link_row(title: &str, subtitle: &str, url: &str, trailing: Vec<Button>) -> ListBoxRow {
    let title_lbl = Label::new(Some(title));
    title_lbl.set_xalign(0.0);
    title_lbl.set_ellipsize(EllipsizeMode::End);
    title_lbl.style_context().add_class("nyx-list-title");

    let sub_lbl = Label::new(Some(subtitle));
    sub_lbl.set_xalign(0.0);
    sub_lbl.set_ellipsize(EllipsizeMode::End);
    sub_lbl.style_context().add_class("nyx-list-sub");

    let text_box = GtkBox::new(Orientation::Vertical, 2);
    text_box.pack_start(&title_lbl, false, false, 0);
    text_box.pack_start(&sub_lbl, false, false, 0);

    let hbox = GtkBox::new(Orientation::Horizontal, 6);
    hbox.pack_start(&text_box, true, true, 0);
    for btn in &trailing {
        hbox.pack_end(btn, false, false, 0);
    }

    let row = ListBoxRow::new();
    row.style_context().add_class("nyx-list-row");
    row.set_widget_name(url);
    row.set_activatable(!url.is_empty());
    row.add(&hbox);
    row
}
