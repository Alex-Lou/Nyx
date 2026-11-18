//! Application : fenêtre tao, WebView du chrome, onglets, boucle
//! d'événements. Toute mutation d'état passe par `App::handle` (thread UI),
//! alimenté par les `UserEvent` postés depuis les callbacks WebView2.
//!
//! Phase 2 : un champ `vault` (coffre SQLCipher) viendra ici ; les
//! événements `PageLoad`/`Title` sont les points d'accroche de l'historique.

use std::path::PathBuf;

use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Icon, Theme, Window, WindowBuilder};
use wry::{NewWindowResponse, Rect, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

use crate::ipc::{self, ChromeMsg, ChromeState};
use crate::keys::Shortcut;
use crate::nav::{self, NEWTAB_URL};
use crate::tabs::{TabId, Tabs};
use crate::{adblock, chrome, content, protocol, webview2};

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
}

/// Lance Nyx. Ne rend la main qu'en cas d'erreur d'initialisation.
pub fn run() -> Result<(), String> {
    adblock::init_in_background();

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
    let mut app = App { tabs: Tabs::new(), chrome, context, window, proxy, last_state: None };
    app.open_tab(NEWTAB_URL, true).map_err(|e| format!("WebView2 : {e}"))?;
    let mut app = Some(app);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                // Libère WebView2 proprement avant la sortie du processus.
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
                if let Some(app) = &mut app {
                    app.handle(ev);
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
    const PNG: &[u8] = include_bytes!("../../../assets/icons/nyx.png");
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
/// navigation, popup, téléchargement ni permission.
fn build_chrome(
    window: &Window,
    context: &mut WebContext,
    proxy: &EventLoopProxy<UserEvent>,
) -> wry::Result<WebView> {
    let ipc_proxy = proxy.clone();
    let view = WebViewBuilder::new_with_web_context(context)
        .with_url(nav::CHROME_URL)
        .with_bounds(rect(0.0, 0.0, 1280.0, chrome::HEIGHT))
        .with_background_color(content::BACKGROUND)
        .with_https_scheme(true)
        .with_additional_browser_args(content::BROWSER_ARGS)
        .with_theme(wry::Theme::Dark)
        .with_devtools(cfg!(debug_assertions))
        .with_hotkeys_zoom(false)
        .with_browser_accelerator_keys(false)
        .with_default_context_menus(false)
        .with_custom_protocol("nyxui".into(), |_, request| {
            protocol::respond(protocol::chrome_asset, &request)
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
    /// Zone de contenu (sous le chrome), en pixels logiques.
    fn content_bounds(&self) -> Rect {
        let size: LogicalSize<f64> = self.window.inner_size().to_logical(self.window.scale_factor());
        rect(0.0, chrome::HEIGHT, size.width, size.height - chrome::HEIGHT)
    }

    fn layout(&self) {
        let size: LogicalSize<f64> = self.window.inner_size().to_logical(self.window.scale_factor());
        let _ = self.chrome.set_bounds(rect(0.0, 0.0, size.width, chrome::HEIGHT));
        let bounds = self.content_bounds();
        for tab in self.tabs.iter() {
            let _ = tab.view.set_bounds(bounds);
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

    fn handle(&mut self, event: UserEvent) {
        match event {
            UserEvent::Chrome(msg) => self.on_chrome(msg),
            UserEvent::PageLoad { tab, loading } => {
                if let Some(t) = self.tabs.get_mut(tab) {
                    t.loading = loading;
                }
                self.refresh(tab);
            }
            UserEvent::Title { tab, title } => {
                if let Some(t) = self.tabs.get_mut(tab) {
                    t.title = title;
                }
                self.push_state();
            }
            UserEvent::History { tab } => self.refresh(tab),
            UserEvent::Load { tab, url } => {
                if let Some(t) = self.tabs.get(tab) {
                    let _ = t.view.load_url(&url);
                }
            }
            UserEvent::OpenTab { url } => {
                if self.open_tab(&url, true).is_err() {
                    self.toast("Impossible d'ouvrir un onglet");
                }
            }
            UserEvent::Download { name, blocked } => {
                let prefix = if blocked { "Téléchargement bloqué" } else { "Téléchargement" };
                self.toast(&format!("{prefix} : {name}"));
            }
            UserEvent::Shortcut(s) => self.on_shortcut(s),
        }
    }

    fn on_chrome(&mut self, msg: ChromeMsg) {
        match msg {
            ChromeMsg::Ready => {
                self.last_state = None;
                self.push_state();
            }
            ChromeMsg::Navigate { text } => match nav::resolve_address(&text) {
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
                if let Some(view) = self.active_view() {
                    let _ = view.load_url(NEWTAB_URL);
                }
            }
            ChromeMsg::NewTab => self.new_tab(),
            ChromeMsg::CloseTab { id } => self.close(id),
            ChromeMsg::SelectTab { id } => self.select(id),
            ChromeMsg::Downloads => self.active_view().into_iter().for_each(webview2::open_downloads),
        }
    }

    fn on_shortcut(&mut self, shortcut: Shortcut) {
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
        let state = ChromeState {
            tabs: self.tabs.snapshot(),
            url: active.map(|t| nav::display_url(&t.url)).unwrap_or_default(),
            loading: active.is_some_and(|t| t.loading),
            can_go_back: active.is_some_and(|t| t.can_go_back),
            can_go_forward: active.is_some_and(|t| t.can_go_forward),
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
