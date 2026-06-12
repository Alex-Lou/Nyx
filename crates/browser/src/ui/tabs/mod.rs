use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib::WeakRef;
use gtk::prelude::*;
use gtk::{ApplicationWindow, Notebook, Widget, Window};
use vault::Vault;
use webkit2gtk::{
    UserContentManager, UserContentManagerExt, WebContext, WebContextExt,
    WebView, WebViewExt,
};

use crate::pages::{self, newtab};
use crate::state::bookmarks::Bookmarks;
use crate::state::settings::Settings;
use crate::ui::{downloads, settings_window};
use crate::web::{self, darkmode, nyxguard::NyxGuard};

mod favicon;
mod label;

type WebViewHook = Rc<RefCell<Box<dyn Fn(&WebView)>>>;

/// Gestion des onglets. `Clone` est cheap (champs ref-comptés) → les closures
/// GTK capturent une `TabBar` clonée sans coût mémoire significatif.
#[derive(Clone)]
pub struct TabBar {
    pub notebook:   Notebook,
    blocker:        Rc<NyxGuard>,
    settings:       Settings,
    bookmarks:      Bookmarks,
    vault:          Rc<Vault>,
    on_new_webview: WebViewHook,
    settings_modal: Rc<RefCell<Option<Window>>>,
    parent:         Rc<RefCell<Option<WeakRef<Window>>>>,
}

impl TabBar {
    pub fn new(blocker: Rc<NyxGuard>, settings: Settings, bm: Bookmarks, vault: Rc<Vault>) -> Self {
        let notebook = Notebook::builder().scrollable(true).show_border(false).build();
        Self {
            notebook, blocker, settings, bookmarks: bm, vault,
            on_new_webview: Rc::new(RefCell::new(Box::new(|_| {}))),
            settings_modal: Rc::new(RefCell::new(None)),
            parent:         Rc::new(RefCell::new(None)),
        }
    }

    /// Mémorise la fenêtre principale (weak) pour ancrer le modal Paramètres.
    pub fn set_parent(&self, w: &ApplicationWindow) {
        *self.parent.borrow_mut() = Some(w.clone().upcast::<Window>().downgrade());
    }

    /// Hook appelé après création de chaque WebView (URL bar + progress câblés
    /// une seule fois par la fenêtre).
    pub fn set_on_new_webview<F: Fn(&WebView) + 'static>(&self, cb: F) {
        *self.on_new_webview.borrow_mut() = Box::new(cb);
    }

    pub fn with_current<F: Fn(&WebView)>(&self, f: F) {
        if let Some(wv) = self.current_webview() { f(&wv); }
    }

    pub fn open_new_tab(&self) -> WebView {
        let wv = self.build_webview(None);
        wv.load_html(&newtab::html(), Some(&pages::assets_base_uri()));
        self.attach(&wv, "Nouvel onglet");
        wv
    }

    /// Ouvre les paramètres dans un modal flottant (déplaçable/redimensionnable).
    /// Single-instance : un nouveau clic ramène la fenêtre existante au premier plan.
    pub fn open_settings(&self) {
        if let Some(w) = self.settings_modal.borrow().as_ref() {
            w.present();
            return;
        }
        let parent = self.parent.borrow().as_ref().and_then(WeakRef::upgrade);
        let modal = settings_window::build(
            parent.as_ref(),
            self.blocker.clone(),
            self.settings.clone(),
            self.bookmarks.clone(),
            self.vault.clone(),
        );
        let slot = self.settings_modal.clone();
        modal.connect_destroy(move |_| { *slot.borrow_mut() = None; });
        *self.settings_modal.borrow_mut() = Some(modal.clone());
        modal.show_all();
    }

    /// Ouvre la page d'accueil configurée (par défaut : nouvel onglet Nyx).
    pub fn open_home(&self) -> WebView {
        let url = self.settings.borrow().home_url.clone();
        if url.is_empty() || url == "nyx://newtab" {
            return self.open_new_tab();
        }
        let wv = self.build_webview(None);
        wv.load_uri(&url);
        self.attach(&wv, "Chargement…");
        wv
    }

    /// Ouvre une URL dans un nouvel onglet (ex. clic sur un favori).
    pub fn open_url(&self, url: &str) -> WebView {
        let wv = self.build_webview(None);
        wv.load_uri(url);
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

    /// Onglet enfant lié à `parent` (signal `create-web-view`).
    fn open_related(&self, parent: &WebView) -> WebView {
        let wv = self.build_webview(Some(parent));
        self.attach(&wv, "Chargement…");
        wv
    }

    fn build_webview(&self, parent: Option<&WebView>) -> WebView {
        let wv = match parent {
            Some(p) => WebView::with_related_view(p),
            None => {
                // Mode privé : contexte éphémère (rien sur disque — cookies,
                // cache, historique vivent en RAM et disparaissent à la fermeture).
                let ctx = if self.settings.borrow().private_mode {
                    WebContext::new_ephemeral()
                } else {
                    WebContext::new()
                };
                // SÉCURITÉ : bac à sable des processus web (avant le 1er WebView).
                // bubblewrap segfault sous WSL (pas de bus-proxy) → actif
                // partout SAUF là. Sur un vrai Linux, le sandbox reste ON.
                if !crate::platform::is_wsl() {
                    ctx.set_sandbox_enabled(true);
                }
                // Téléchargements : chaque contexte doit être branché.
                downloads::wire_context(&ctx);

                // Mode sombre forcé : injecté via un UserContentManager dédié.
                let ucm = UserContentManager::new();
                if self.settings.borrow().dark_websites {
                    ucm.add_style_sheet(&darkmode::stylesheet());
                }

                WebView::builder()
                    .web_context(&ctx)
                    .user_content_manager(&ucm)
                    .build()
            }
        };
        wv.set_vexpand(true);
        wv.set_hexpand(true);
        web::configure(
            &wv,
            self.blocker.clone(),
            self.settings.clone(),
            self.bookmarks.clone(),
            self.vault.clone(),
        );

        // Liens target=_blank / window.open → nouvel onglet.
        let tabs = self.clone();
        wv.connect_create(move |opener, _| {
            Some(tabs.open_related(opener).upcast::<Widget>())
        });

        (self.on_new_webview.borrow())(&wv);
        wv
    }

    fn attach(&self, wv: &WebView, initial: &str) {
        let tab = label::build(wv, &self.notebook, initial);
        let idx = self.notebook.append_page(wv, Some(&tab));
        self.notebook.set_tab_reorderable(wv, true);
        self.notebook.set_current_page(Some(idx));
        wv.show();
    }
}
