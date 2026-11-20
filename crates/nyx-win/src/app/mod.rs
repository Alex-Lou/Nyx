//! Application : fenêtre tao, WebView du chrome, onglets, boucle
//! d'événements. Toute mutation d'état passe par `App::handle` (thread UI),
//! alimenté par les `UserEvent` postés depuis les callbacks WebView2.
//!
//! Le coffre (déverrouillage, capture, historique, pages internes, imports)
//! vit dans le sous-module `vault`. Tant qu'il est verrouillé, le chrome
//! couvre toute la fenêtre (écran de déverrouillage) et aucun onglet n'existe.

mod vault;

use std::path::PathBuf;

use nyx_core::security::Page;
use nyx_core::state::bookmarks::{self as bookmarks_state, Bookmarks};
use nyx_core::state::settings::{self as settings_state, Settings};
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Icon, Theme, Window, WindowBuilder};
use wry::{NewWindowResponse, Rect, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

use crate::ipc::{self, ChromeMsg, ChromeState, Overlay};
use crate::keys::Shortcut;
use crate::nav::{self, Action, NEWTAB_URL};
use crate::paths::{self, Capsule};
use crate::secret::Secret;
use crate::tabs::{TabId, Tabs};
use crate::{adblock, chrome, content, history, protocol, webview2};

/// Réponse différée du protocole `nyx` (page rendue sur le thread UI).
pub struct Responder(pub wry::RequestAsyncResponder);

impl std::fmt::Debug for Responder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Responder")
    }
}

/// Événement posté à la boucle principale.
#[derive(Debug)]
pub enum UserEvent {
    /// Commande validée venant du chrome.
    Chrome(ChromeMsg),
    PageLoad { tab: TabId, loading: bool },
    Title { tab: TabId, title: String },
    /// URL ou historique changé : relire url / précédent / suivant.
    History { tab: TabId },
    /// Charger `url` dans l'onglet (redirection décidée par `nav`).
    Load { tab: TabId, url: String },
    /// `target=_blank` / `window.open` → nouvel onglet actif.
    OpenTab { url: String },
    Download { name: String, blocked: bool },
    Shortcut(Shortcut),
    /// Action interne autorisée (page interne émettrice, `security::decide`).
    Action { tab: TabId, action: Action },
    /// Identifiants soumis sur `host` (origine déjà vérifiée).
    Credential { host: String, username: String, password: Secret },
    /// Page interne à rendre puis servir.
    Serve { page: Page, responder: Responder },
    /// Résultat d'une sonde de la capsule WSL.
    Capsule(Capsule),
}

/// Question Oui/Non en attente (enregistrement d'un mot de passe).
struct Pending {
    id: u32,
    text: String,
    host: String,
    username: String,
    password: Secret,
    plan: crate::capture::Plan,
}

/// Ordre des champs = ordre de destruction : WebView avant la fenêtre.
struct App {
    tabs: Tabs<WebView>,
    chrome: WebView,
    context: WebContext,
    window: Window,
    proxy: EventLoopProxy<UserEvent>,
    /// Dernier état poussé au chrome (évite les scripts redondants).
    last_state: Option<ChromeState>,
    /// Coffre ouvert (`None` = écran de déverrouillage).
    vault: Option<::vault::Vault>,
    vault_path: PathBuf,
    settings: Settings,
    bookmarks: Bookmarks,
    recorder: history::Recorder,
    prompt: Option<Pending>,
    next_prompt: u32,
    overlay: Option<Overlay>,
    /// Dernière sonde de la capsule (`None` = en cours).
    capsule: Option<Capsule>,
}

/// Lance Nyx. Ne rend la main qu'en cas d'erreur d'initialisation.
pub fn run() -> Result<(), String> {
    adblock::init_in_background();
    let appdata = std::env::var("APPDATA").ok();
    let vault_path = paths::vault_path(appdata.as_deref())
        .map(PathBuf::from)
        .ok_or("variable APPDATA absente : emplacement du coffre inconnu")?;

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Nyx")
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(480.0, 320.0))
        .with_theme(Some(Theme::Dark))
        .with_window_icon(window_icon())
        .build(&event_loop)
        .map_err(|e| format!("fenêtre : {e}"))?;

    let mut context = WebContext::new(data_dir());
    let chrome = build_chrome(&window, &mut context, &proxy).map_err(|e| format!("WebView2 : {e}"))?;
    let mut app = App {
        tabs: Tabs::new(),
        chrome,
        context,
        window,
        proxy,
        last_state: None,
        vault: None,
        vault_path,
        settings: settings_state::new(),
        bookmarks: bookmarks_state::new(),
        recorder: history::Recorder::default(),
        prompt: None,
        next_prompt: 1,
        // Écran plein (vide) jusqu'à la décision de `start_vault`.
        overlay: Some(Overlay::Unlock { notice: String::new(), error: String::new() }),
        capsule: None,
    };
    app.layout();
    app.start_vault();
    let mut app = Some(app);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                // Libère WebView2 (et ferme le coffre) avant la sortie.
                drop(app.take());
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. },
                ..
            } => {
                if let Some(app) = &app {
                    app.layout();
                }
            }
            Event::UserEvent(ev) => {
                let keep = app.as_mut().is_none_or(|a| a.handle(ev));
                if !keep {
                    drop(app.take());
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    })
}

/// `%LOCALAPPDATA%\Nyx\WebView2` (profil, cookies, cache partagés).
fn data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Nyx").join("WebView2"))
}

fn window_icon() -> Option<Icon> {
    const PNG: &[u8] = include_bytes!("../../../../assets/icons/nyx.png");
    let img = image::load_from_memory_with_format(PNG, image::ImageFormat::Png).ok()?;
    let img = img.resize(64, 64, image::imageops::FilterType::Lanczos3).into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect {
        position: LogicalPosition::new(x, y).into(),
        size: LogicalSize::new(w.max(0.0), h.max(0.0)).into(),
    }
}

/// WebView du chrome : origine `nyxui` figée, IPC validé, aucune
/// navigation, popup, téléchargement ni permission. Seul canal par lequel
/// une passphrase peut entrer dans Nyx.
fn build_chrome(
    window: &Window,
    context: &mut WebContext,
    proxy: &EventLoopProxy<UserEvent>,
) -> wry::Result<WebView> {
    let ipc_proxy = proxy.clone();
    let view = WebViewBuilder::new_with_web_context(context)
        .with_url(nav::CHROME_URL)
        .with_bounds(rect(0.0, 0.0, 1280.0, 800.0))
        .with_background_color(content::BACKGROUND)
        .with_https_scheme(true)
        .with_additional_browser_args(content::BROWSER_ARGS)
        .with_theme(wry::Theme::Dark)
        .with_devtools(cfg!(debug_assertions))
        .with_hotkeys_zoom(false)
        .with_browser_accelerator_keys(false)
        .with_default_context_menus(false)
        .with_custom_protocol("nyxui".into(), |_, request| {
            let asset = match protocol::target(&request) {
                Some((host, path)) => protocol::chrome_asset(&host, &path),
                None => protocol::method_not_allowed(),
            };
            protocol::to_response(asset)
        })
        .with_ipc_handler(move |request| {
            if !nav::is_chrome(&request.uri().to_string()) {
                return;
            }
            if let Some(msg) = ChromeMsg::parse(request.body()) {
                let _ = ipc_proxy.send_event(UserEvent::Chrome(msg));
            }
        })
        .with_navigation_handler(|target| nav::chrome_may_navigate(&target))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_permission_handler(|_| wry::PermissionResponse::Deny)
        .build_as_child(window)?;
    let _ = webview2::install_shortcuts(&view, proxy.clone());
    Ok(view)
}

impl App {
    fn window_size(&self) -> LogicalSize<f64> {
        self.window.inner_size().to_logical(self.window.scale_factor())
    }

    /// Hauteur du chrome : toute la fenêtre sous un panneau (déverrouillage,
    /// import), sinon barre + question éventuelle.
    fn chrome_height(&self) -> f64 {
        if self.overlay.is_some() {
            self.window_size().height
        } else if self.prompt.is_some() {
            chrome::HEIGHT + chrome::PROMPT_HEIGHT
        } else {
            chrome::HEIGHT
        }
    }

    /// Zone de contenu (sous le chrome), en pixels logiques.
    fn content_bounds(&self) -> Rect {
        let size = self.window_size();
        let top = self.chrome_height().min(size.height);
        rect(0.0, top, size.width, size.height - top)
    }

    fn layout(&self) {
        let size = self.window_size();
        let _ = self.chrome.set_bounds(rect(0.0, 0.0, size.width, self.chrome_height()));
        let bounds = self.content_bounds();
        for tab in self.tabs.iter() {
            let _ = tab.view.set_bounds(bounds);
        }
        // Un panneau plein écran masque l'onglet actif (les WebView enfants
        // créées après le chrome passent devant lui).
        if let Some(tab) = self.tabs.active() {
            let _ = tab.view.set_visible(self.overlay.is_none());
        }
    }

    fn open_tab(&mut self, url: &str, activate: bool) -> wry::Result<()> {
        let id = self.tabs.next_id();
        let bounds = self.content_bounds();
        let view = content::build(&self.window, &mut self.context, &self.proxy, id, url, bounds, activate)?;
        self.tabs.open(view, url);
        if activate {
            self.select(id);
        }
        self.push_state();
        Ok(())
    }

    fn select(&mut self, id: TabId) {
        let Some(previous) = self.tabs.activate(id) else { return };
        if let Some(old) = previous.and_then(|p| self.tabs.get(p)) {
            let _ = old.view.set_visible(false);
        }
        if let Some(tab) = self.tabs.get(id) {
            let _ = tab.view.set_visible(true);
            let _ = tab.view.focus();
        }
        self.push_state();
    }

    fn close(&mut self, id: TabId) {
        // La WebView rendue est détruite ici : processus de rendu libéré.
        drop(self.tabs.close(id));
        self.recorder.forget(id);
        if self.tabs.is_empty() {
            self.new_tab();
            return;
        }
        if let Some(tab) = self.tabs.active() {
            let _ = tab.view.set_visible(true);
            let _ = tab.view.focus();
        }
        self.push_state();
    }

    fn new_tab(&mut self) {
        if self.open_tab(NEWTAB_URL, true).is_err() {
            self.toast("Impossible d'ouvrir un onglet");
        }
    }

    fn active_view(&self) -> Option<&WebView> {
        self.tabs.active().map(|t| &t.view)
    }

    /// Traite un événement. `false` = quitter Nyx.
    fn handle(&mut self, event: UserEvent) -> bool {
        match event {
            UserEvent::Chrome(msg) => return self.on_chrome(msg),
            UserEvent::PageLoad { tab, loading } => {
                if let Some(t) = self.tabs.get_mut(tab) {
                    t.loading = loading;
                }
                self.refresh(tab);
                if !loading {
                    self.after_load(tab);
                }
            }
            UserEvent::Title { tab, title } => {
                if let Some(t) = self.tabs.get_mut(tab) {
                    t.title = title;
                }
                self.retitle(tab);
                self.push_state();
            }
            UserEvent::History { tab } => self.refresh(tab),
            UserEvent::Load { tab, url } => {
                if let Some(t) = self.tabs.get(tab) {
                    let _ = t.view.load_url(&url);
                }
            }
            UserEvent::OpenTab { url } => {
                if self.vault.is_some() && self.open_tab(&url, true).is_err() {
                    self.toast("Impossible d'ouvrir un onglet");
                }
            }
            UserEvent::Download { name, blocked } => {
                let prefix = if blocked { "Téléchargement bloqué" } else { "Téléchargement" };
                self.toast(&format!("{prefix} : {name}"));
            }
            UserEvent::Shortcut(s) => self.on_shortcut(s),
            UserEvent::Action { tab, action } => self.on_action(tab, action),
            UserEvent::Credential { host, username, password } => self.on_credential(host, username, password),
            UserEvent::Serve { page, responder } => self.serve(page, responder),
            UserEvent::Capsule(status) => self.on_capsule_probed(status),
        }
        true
    }

    fn on_chrome(&mut self, msg: ChromeMsg) -> bool {
        if self.vault.is_none() && !msg.allowed_while_locked() {
            return true;
        }
        match msg {
            ChromeMsg::Ready => {
                self.last_state = None;
                self.push_state();
            }
            ChromeMsg::Navigate { text } => match nav::resolve_address(&text, &self.settings.borrow()) {
                Some(url) => {
                    if let Some(view) = self.active_view() {
                        let _ = view.load_url(&url);
                        let _ = view.focus();
                    }
                }
                None => self.toast("Adresse non autorisée"),
            },
            ChromeMsg::Back => self.active_view().into_iter().for_each(webview2::go_back),
            ChromeMsg::Forward => self.active_view().into_iter().for_each(webview2::go_forward),
            ChromeMsg::Stop => self.active_view().into_iter().for_each(webview2::stop),
            ChromeMsg::Reload => {
                if let Some(view) = self.active_view() {
                    let _ = view.reload();
                }
            }
            ChromeMsg::Home => {
                let home = nav::home_url(&self.settings.borrow());
                if let Some(view) = self.active_view() {
                    let _ = view.load_url(&home);
                }
            }
            ChromeMsg::NewTab => self.new_tab(),
            ChromeMsg::CloseTab { id } => self.close(id),
            ChromeMsg::SelectTab { id } => self.select(id),
            ChromeMsg::Downloads => self.active_view().into_iter().for_each(webview2::open_downloads),
            ChromeMsg::ToggleBookmark => self.toggle_bookmark(),
            ChromeMsg::PromptAnswer { id, yes } => self.on_prompt_answer(id, yes),
            ChromeMsg::Unlock { passphrase } => self.on_unlock(passphrase),
            ChromeMsg::Create { master, passphrase, confirm, import_capsule, capsule_passphrase } => {
                self.on_create(master, passphrase, confirm, import_capsule, capsule_passphrase)
            }
            ChromeMsg::ImportCapsule { passphrase } => self.on_import_capsule(passphrase),
            ChromeMsg::SetMaster { passphrase, confirm } => self.on_set_master(passphrase, confirm),
            ChromeMsg::ChangeMaster { old, passphrase, confirm } => self.on_change_master(old, passphrase, confirm),
            ChromeMsg::RemoveMaster { passphrase } => self.on_remove_master(passphrase),
            ChromeMsg::RetryCapsule => self.probe_capsule_again(),
            ChromeMsg::CloseOverlay => {
                if self.vault.is_some() && self.overlay.as_ref().is_some_and(|o| !o.is_locked_screen()) {
                    self.set_overlay(None);
                }
            }
            ChromeMsg::Quit => return self.vault.is_some(),
        }
        true
    }

    fn on_shortcut(&mut self, shortcut: Shortcut) {
        if self.vault.is_none() || self.overlay.is_some() {
            return;
        }
        match shortcut {
            Shortcut::NewTab => self.new_tab(),
            Shortcut::CloseTab => {
                if let Some(id) = self.tabs.active_id() {
                    self.close(id);
                }
            }
            Shortcut::FocusUrl => {
                let _ = self.chrome.focus();
                let _ = self.chrome.evaluate_script(ipc::FOCUS_URLBAR_SCRIPT);
            }
            Shortcut::NextTab | Shortcut::PrevTab => {
                if let Some(id) = self.tabs.neighbour(shortcut == Shortcut::NextTab) {
                    self.select(id);
                }
            }
        }
    }

    /// Relit url / précédent / suivant d'un onglet depuis WebView2.
    fn refresh(&mut self, id: TabId) {
        if let Some(tab) = self.tabs.get_mut(id) {
            if let Ok(url) = tab.view.url() {
                tab.url = url;
            }
            (tab.can_go_back, tab.can_go_forward) = webview2::history_state(&tab.view);
        }
        self.push_state();
    }

    /// Pousse l'état au chrome s'il a changé.
    fn push_state(&mut self) {
        let active = self.tabs.active();
        let url = active.map(|t| t.url.as_str()).unwrap_or("");
        let state = ChromeState {
            tabs: self.tabs.snapshot(),
            url: active.map(|t| nav::display_url(&t.url)).unwrap_or_default(),
            loading: active.is_some_and(|t| t.loading),
            can_go_back: active.is_some_and(|t| t.can_go_back),
            can_go_forward: active.is_some_and(|t| t.can_go_forward),
            bookmarked: self.bookmarks.borrow().iter().any(|b| b.url == url),
            can_bookmark: self.vault.is_some() && history::recordable(url),
            prompt: self.prompt.as_ref().map(|p| ipc::PromptView { id: p.id, text: p.text.clone() }),
            overlay: self.overlay.clone(),
            capsule_cmd: paths::START_CAPSULE_CMD,
        };
        if self.last_state.as_ref() == Some(&state) {
            return;
        }
        let _ = self.chrome.evaluate_script(&ipc::update_script(&state));
        self.last_state = Some(state);
    }

    fn toast(&self, text: &str) {
        let _ = self.chrome.evaluate_script(&ipc::toast_script(text));
    }
}
