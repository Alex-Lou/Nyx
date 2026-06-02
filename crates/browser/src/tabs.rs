use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gtk::cairo;
use gtk::gdk_pixbuf::InterpType;
use gtk::pango::EllipsizeMode;
use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, IconSize, Image, Label, Notebook, Orientation, Widget};
use webkit2gtk::{WebContext, WebView, WebViewExt};

/// Taille (px) de la favicon dans l'onglet — alignée sur IconSize::Menu (16).
const FAVICON_PX: i32 = 16;

use crate::adblock::AdBlocker;
use crate::{newtab, webview};

/// Barre d'onglets : création, fermeture, navigation entre onglets.
///
/// L'état mutable (le hook `on_new_webview`) vit dans `Rc<RefCell<…>>` pour
/// pouvoir être partagé entre la fenêtre parente et les closures GTK.
/// Les autres champs (`notebook`, `blocker`) sont déjà ref-comptés, donc
/// dériver `Clone` est cheap et permet de capturer la `TabBar` complète
/// dans les callbacks de boutons / raccourcis.
#[derive(Clone)]
pub struct TabBar {
    pub notebook: Notebook,
    blocker: Arc<AdBlocker>,
    on_new_webview: Rc<RefCell<Box<dyn Fn(&WebView)>>>,
}

impl TabBar {
    pub fn new(blocker: Arc<AdBlocker>) -> Self {
        let notebook = Notebook::builder()
            .scrollable(true)
            .show_border(false)
            .build();
        Self {
            notebook,
            blocker,
            on_new_webview: Rc::new(RefCell::new(Box::new(|_| {}))),
        }
    }

    /// Installe un hook appelé après la création de chaque WebView. La fenêtre
    /// l'utilise pour câbler URL bar / progress bar par onglet, une seule fois,
    /// au lieu de re-brancher les signaux à chaque changement d'onglet.
    pub fn set_on_new_webview<F: Fn(&WebView) + 'static>(&self, cb: F) {
        *self.on_new_webview.borrow_mut() = Box::new(cb);
    }

    /// Ouvre un onglet vide → Nyx start page.
    pub fn open_new_tab(&self) -> WebView {
        let wv = self.build_webview(None);
        wv.load_html(newtab::html(), Some(newtab::URI));
        self.attach_tab(&wv, "Nouvel onglet");
        wv
    }

    /// Ouvre un onglet sur une URL.
    pub fn open(&self, url: &str) -> WebView {
        let wv = self.build_webview(None);
        wv.load_uri(url);
        self.attach_tab(&wv, "Chargement…");
        wv
    }

    /// Ouvre un onglet relié à `parent` — utilisé par le signal create pour
    /// les ouvertures en nouvel onglet (target="_blank", window.open, clic
    /// molette). WebKit y charge l'URL lui-même via la related-view.
    pub fn open_related(&self, parent: &WebView) -> WebView {
        let wv = self.build_webview(Some(parent));
        self.attach_tab(&wv, "Chargement…");
        wv
    }

    pub fn current_webview(&self) -> Option<WebView> {
        let page = self.notebook.current_page()?;
        self.notebook
            .nth_page(Some(page))?
            .downcast::<WebView>()
            .ok()
    }

    pub fn close_current(&self) {
        if let Some(page) = self.notebook.current_page() {
            self.notebook.remove_page(Some(page));
        }
    }

    /// Cycle vers l'onglet suivant ; revient au premier après le dernier.
    pub fn next_tab(&self) {
        let n = self.notebook.n_pages();
        if n == 0 {
            return;
        }
        let cur = self.notebook.current_page().unwrap_or(0);
        self.notebook.set_current_page(Some((cur + 1) % n));
    }

    /// Crée une WebView, applique les réglages privacy + adblock, branche
    /// `connect_create` pour les ouvertures en nouvel onglet, puis notifie
    /// l'observeur. Pas encore attachée au notebook — c'est `attach_tab` qui
    /// s'en charge.
    ///
    /// Si `parent` est fourni, la WebView est créée via `with_related_view`
    /// pour que WebKit pilote sa navigation (signal create).
    fn build_webview(&self, parent: Option<&WebView>) -> WebView {
        let wv = if let Some(p) = parent {
            WebView::with_related_view(p)
        } else {
            // Onglet racine → son propre WebContext.
            // WebKitGTK 4.1 ne tolère qu'un seul WebContext non-éphémère par
            // process, donc l'isolation reste partielle (cookies/cache de
            // session séparés, disque partagé). Sprint 5 passera à
            // `WebContext::new_ephemeral()` pour cloisonner vraiment.
            let context = WebContext::new();
            WebView::builder().web_context(&context).build()
        };
        wv.set_vexpand(true);
        wv.set_hexpand(true);
        webview::configure(&wv, self.blocker.clone());

        // Ticket 1.7 : liens externes → nouvel onglet.
        {
            let tabs = self.clone();
            wv.connect_create(move |opener, _nav_action| {
                Some(tabs.open_related(opener).upcast::<Widget>())
            });
        }

        (self.on_new_webview.borrow())(&wv);
        wv
    }

    fn attach_tab(&self, webview: &WebView, initial: &str) {
        let label = build_tab_label(webview, &self.notebook, initial);
        let idx = self.notebook.append_page(webview, Some(&label));
        self.notebook.set_tab_reorderable(webview, true);
        self.notebook.set_current_page(Some(idx));
        webview.show();
    }
}

fn build_tab_label(webview: &WebView, notebook: &Notebook, initial: &str) -> GtkBox {
    let favicon = Image::new();
    favicon.set_pixel_size(FAVICON_PX);
    favicon.style_context().add_class("nyx-tab-favicon");
    set_fallback_favicon(&favicon);

    let label = Label::new(Some(initial));
    label.set_max_width_chars(20);
    label.set_ellipsize(EllipsizeMode::End);

    let close = Button::with_label("×");
    close.style_context().add_class("nyx-tab-close");
    close.set_relief(gtk::ReliefStyle::None);

    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.pack_start(&favicon, false, false, 0);
    row.pack_start(&label,   true,  true,  0);
    row.pack_start(&close,   false, false, 0);
    row.show_all();

    // Titre dynamique (Ticket 1.2)
    {
        let lbl = label.clone();
        webview.connect_title_notify(move |wv| {
            let t = wv
                .title()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Nouvel onglet".into());
            lbl.set_text(&t);
            lbl.set_tooltip_text(Some(&t));
        });
    }

    // Favicon dynamique (Ticket 1.3)
    {
        let img = favicon.clone();
        webview.connect_favicon_notify(move |wv| {
            update_favicon(&img, wv);
        });
    }

    // Bouton fermeture
    {
        let nb = notebook.clone();
        let wv = webview.clone();
        close.connect_clicked(move |_| {
            if let Some(idx) = nb.page_num(&wv) {
                nb.remove_page(Some(idx));
            }
        });
    }

    row
}

/// Convertit la `cairo::Surface` exposée par WebKit en `Pixbuf` 16×16
/// et l'affiche dans l'`Image` ; tombe sur l'icône globe par défaut si la
/// favicon est absente ou non-image.
fn update_favicon(img: &Image, wv: &WebView) {
    let Some(surface) = wv.favicon() else {
        set_fallback_favicon(img);
        return;
    };
    let Ok(image_surf) = cairo::ImageSurface::try_from(surface) else {
        set_fallback_favicon(img);
        return;
    };
    let w = image_surf.width();
    let h = image_surf.height();
    if w <= 0 || h <= 0 {
        set_fallback_favicon(img);
        return;
    }
    let Some(pixbuf) = gtk::gdk::pixbuf_get_from_surface(&image_surf, 0, 0, w, h) else {
        set_fallback_favicon(img);
        return;
    };
    let scaled = pixbuf
        .scale_simple(FAVICON_PX, FAVICON_PX, InterpType::Bilinear)
        .unwrap_or(pixbuf);
    img.set_from_pixbuf(Some(&scaled));
}

fn set_fallback_favicon(img: &Image) {
    img.set_from_icon_name(Some("text-html-symbolic"), IconSize::Menu);
}
