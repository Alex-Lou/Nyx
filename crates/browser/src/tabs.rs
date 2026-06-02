use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gtk::pango::EllipsizeMode;
use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Image, Label, Notebook, Orientation, Widget};
use webkit2gtk::{WebContext, WebView, WebViewExt};

use crate::adblock::AdBlocker;
use crate::bookmarks::Bookmarks;
use crate::settings::Settings;
use crate::{favicon, newtab, settings_page, webview};

type WebViewHook = Rc<RefCell<Box<dyn Fn(&WebView)>>>;

#[derive(Clone)]
pub struct TabBar {
    pub notebook:   Notebook,
    blocker:        Arc<AdBlocker>,
    settings:       Settings,
    bookmarks:      Bookmarks,
    on_new_webview: WebViewHook,
}

impl TabBar {
    pub fn new(blocker: Arc<AdBlocker>, settings: Settings, bm: Bookmarks) -> Self {
        let notebook = Notebook::builder().scrollable(true).show_border(false).build();
        Self {
            notebook, blocker, settings, bookmarks: bm,
            on_new_webview: Rc::new(RefCell::new(Box::new(|_| {}))),
        }
    }

    pub fn set_on_new_webview<F: Fn(&WebView) + 'static>(&self, cb: F) {
        *self.on_new_webview.borrow_mut() = Box::new(cb);
    }

    pub fn with_current<F: Fn(&WebView)>(&self, f: F) {
        if let Some(wv) = self.current_webview() { f(&wv); }
    }

    pub fn open_new_tab(&self) -> WebView {
        let wv = self.build_webview(None);
        wv.load_html(newtab::html(), Some(&newtab::base_uri()));
        self.attach(&wv, "Nouvel onglet");
        wv
    }

    pub fn open_settings(&self) -> WebView {
        let html = settings_page::html(&self.settings.borrow());
        let wv = self.build_webview(None);
        wv.load_html(&html, Some(&settings_page::base_uri()));
        self.attach(&wv, "Paramètres");
        wv
    }

    #[allow(dead_code)]
    pub fn open(&self, url: &str) -> WebView {
        let wv = self.build_webview(None);
        wv.load_uri(url);
        self.attach(&wv, "Chargement…");
        wv
    }

    pub fn open_related(&self, parent: &WebView) -> WebView {
        let wv = self.build_webview(Some(parent));
        self.attach(&wv, "Chargement…");
        wv
    }

    pub fn current_webview(&self) -> Option<WebView> {
        let p = self.notebook.current_page()?;
        self.notebook.nth_page(Some(p))?.downcast::<WebView>().ok()
    }

    pub fn close_current(&self) {
        if let Some(p) = self.notebook.current_page() {
            self.notebook.remove_page(Some(p));
        }
    }

    pub fn next_tab(&self) {
        let n = self.notebook.n_pages();
        if n == 0 { return; }
        let cur = self.notebook.current_page().unwrap_or(0);
        self.notebook.set_current_page(Some((cur + 1) % n));
    }

    fn build_webview(&self, parent: Option<&WebView>) -> WebView {
        let wv = match parent {
            Some(p) => WebView::with_related_view(p),
            None    => {
                let ctx = WebContext::new();
                WebView::builder().web_context(&ctx).build()
            }
        };
        wv.set_vexpand(true);
        wv.set_hexpand(true);
        webview::configure(&wv, self.blocker.clone(), self.settings.clone(), self.bookmarks.clone());

        let tabs = self.clone();
        wv.connect_create(move |opener, _| {
            Some(tabs.open_related(opener).upcast::<Widget>())
        });

        (self.on_new_webview.borrow())(&wv);
        wv
    }

    fn attach(&self, wv: &WebView, initial: &str) {
        let label = build_tab_label(wv, &self.notebook, initial);
        let idx   = self.notebook.append_page(wv, Some(&label));
        self.notebook.set_tab_reorderable(wv, true);
        self.notebook.set_current_page(Some(idx));
        wv.show();
    }
}

fn build_tab_label(wv: &WebView, nb: &Notebook, initial: &str) -> GtkBox {
    let icon = Image::new();
    icon.set_pixel_size(favicon::FAVICON_PX);
    icon.style_context().add_class("nyx-tab-favicon");
    favicon::set_fallback(&icon);
    favicon::bind(&icon, wv);

    let label = Label::new(Some(initial));
    label.set_max_width_chars(20);
    label.set_ellipsize(EllipsizeMode::End);

    let close = Button::with_label("×");
    close.style_context().add_class("nyx-tab-close");
    close.set_relief(gtk::ReliefStyle::None);

    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.pack_start(&icon,  false, false, 0);
    row.pack_start(&label, true,  true,  0);
    row.pack_start(&close, false, false, 0);
    row.show_all();

    let lbl = label.clone();
    wv.connect_title_notify(move |wv| {
        let t = wv.title().map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Nouvel onglet".into());
        lbl.set_text(&t);
        lbl.set_tooltip_text(Some(&t));
    });

    let nb = nb.clone(); let wv = wv.clone();
    close.connect_clicked(move |_| {
        if let Some(i) = nb.page_num(&wv) { nb.remove_page(Some(i)); }
    });
    row
}
