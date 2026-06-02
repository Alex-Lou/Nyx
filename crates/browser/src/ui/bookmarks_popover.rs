use gtk::prelude::*;
use gtk::{
    Box as GtkBox, Button, FileChooserAction, FileChooserNative, Label, ListBox,
    Orientation, Popover, PositionType, ResponseType, ScrolledWindow, Window,
};

use crate::state::bookmarks::{self, Bookmarks};
use crate::ui::tabs::TabBar;

/// Petit gestionnaire de favoris ancré sur le bouton ☆ : liste (ouvrir /
/// supprimer) + import / export au format Netscape (universel).
/// Dossiers + drag-and-drop : itération suivante.
pub fn build(anchor: &Button, tabs: &TabBar, bm: &Bookmarks) -> Popover {
    let pop = Popover::new(Some(anchor));
    pop.set_position(PositionType::Bottom);
    pop.style_context().add_class("nyx-bm-pop");

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_size_request(300, -1);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(10);
    root.set_margin_end(10);

    // En-tête : titre + import / export.
    let header = GtkBox::new(Orientation::Horizontal, 6);
    let title = Label::new(Some("Favoris"));
    title.set_xalign(0.0);
    title.style_context().add_class("nyx-bm-title");
    let import = flat_button("⤓", "Importer (HTML navigateur)");
    let export = flat_button("⤒", "Exporter (HTML navigateur)");
    header.pack_start(&title, true, true, 0);
    header.pack_end(&export, false, false, 0);
    header.pack_end(&import, false, false, 0);

    let list = ListBox::new();
    list.style_context().add_class("nyx-bm-list");
    let scroll = ScrolledWindow::builder().min_content_height(240).build();
    scroll.add(&list);

    root.pack_start(&header, false, false, 0);
    root.pack_start(&scroll, true, true, 0);
    pop.add(&root);
    root.show_all();

    // Reconstruit la liste à chaque ouverture (les favoris changent).
    {
        let (list, tabs, bm, p) = (list.clone(), tabs.clone(), bm.clone(), pop.clone());
        pop.connect_show(move |_| repopulate(&list, &tabs, &bm, &p));
    }
    {
        let (anchor, bm) = (anchor.clone(), bm.clone());
        import.connect_clicked(move |_| import_dialog(&anchor, &bm));
    }
    {
        let bm = bm.clone();
        let anchor = anchor.clone();
        export.connect_clicked(move |_| export_dialog(&anchor, &bm));
    }

    pop
}

fn repopulate(list: &ListBox, tabs: &TabBar, bm: &Bookmarks, pop: &Popover) {
    for child in list.children() {
        list.remove(&child);
    }

    let items = bm.borrow().clone();
    if items.is_empty() {
        let empty = Label::new(Some("Aucun favori"));
        empty.style_context().add_class("nyx-bm-empty");
        empty.set_margin_top(18);
        empty.set_margin_bottom(18);
        list.add(&empty);
        list.show_all();
        return;
    }

    // Affichage groupé : racine d'abord, puis chaque dossier.
    let mut paths = vec![String::new()];
    paths.extend(bookmarks::folders(&items));
    for path in &paths {
        add_group_header(list, path);
        for (i, b) in items.iter().enumerate().filter(|(_, b)| &b.folder == path) {
            add_row(list, tabs, bm, pop, i, b);
        }
    }
    list.show_all();
}

fn add_group_header(list: &ListBox, path: &str) {
    let txt = if path.is_empty() { "Racine".to_string() } else { path.to_string() };
    let label = Label::new(Some(&txt));
    label.set_xalign(0.0);
    label.set_margin_top(8);
    label.style_context().add_class("nyx-bm-title");
    list.add(&label);
}

fn add_row(list: &ListBox, tabs: &TabBar, bm: &Bookmarks, pop: &Popover, i: usize, b: &bookmarks::Bookmark) {
    let row = GtkBox::new(Orientation::Horizontal, 6);
    let label = if b.title.is_empty() { &b.url } else { &b.title };
    let open = Button::with_label(&truncate(label, 36));
    open.set_relief(gtk::ReliefStyle::None);
    open.set_tooltip_text(Some(&b.url));
    if let Some(lbl) = open.child().and_then(|c| c.downcast::<Label>().ok()) {
        lbl.set_xalign(0.0);
    }
    let del = flat_button("×", "Supprimer");
    row.pack_start(&open, true, true, 0);
    row.pack_end(&del, false, false, 0);
    list.add(&row);

    let (t, url, p) = (tabs.clone(), b.url.clone(), pop.clone());
    open.connect_clicked(move |_| { t.open_url(&url); p.popdown(); });

    let (list, tabs, bm, pop) = (list.clone(), tabs.clone(), bm.clone(), pop.clone());
    del.connect_clicked(move |_| {
        bookmarks::remove(&bm, i);
        repopulate(&list, &tabs, &bm, &pop);
    });
}

fn export_dialog(anchor: &Button, bm: &Bookmarks) {
    let parent = toplevel_window(anchor);
    let chooser = FileChooserNative::new(
        Some("Exporter les favoris"),
        parent.as_ref(),
        FileChooserAction::Save,
        Some("Exporter"),
        Some("Annuler"),
    );
    chooser.set_current_name("nyx-bookmarks.html");
    if chooser.run() == ResponseType::Accept {
        if let Some(path) = chooser.filename() {
            let html = bookmarks::export_netscape(&bm.borrow());
            let _ = std::fs::write(path, html);
        }
    }
}

fn import_dialog(anchor: &Button, bm: &Bookmarks) {
    let parent = toplevel_window(anchor);
    let chooser = FileChooserNative::new(
        Some("Importer des favoris"),
        parent.as_ref(),
        FileChooserAction::Open,
        Some("Importer"),
        Some("Annuler"),
    );
    if chooser.run() == ResponseType::Accept {
        if let Some(path) = chooser.filename() {
            if let Ok(html) = std::fs::read_to_string(path) {
                for b in bookmarks::import_netscape(&html) {
                    bookmarks::add_in(bm, b.url, b.title, b.folder);
                }
            }
        }
    }
}

fn toplevel_window(w: &Button) -> Option<Window> {
    w.toplevel().and_then(|t| t.downcast::<Window>().ok())
}

fn flat_button(label: &str, tooltip: &str) -> Button {
    let btn = Button::with_label(label);
    btn.set_relief(gtk::ReliefStyle::None);
    btn.set_tooltip_text(Some(tooltip));
    btn.style_context().add_class("nyx-nav-btn");
    btn
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
