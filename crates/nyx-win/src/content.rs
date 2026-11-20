//! WebView d'un onglet de contenu : construction + câblage des handlers.
//!
//! Chaque handler ne fait que (1) demander la décision à `nav`/`adblock`/
//! `content_ipc` (donc à nyx-core) et (2) poster un `UserEvent` à la boucle
//! principale : aucun accès direct à l'état de l'app depuis un callback
//! WebView2.

use std::rc::Rc;
use std::sync::Mutex;

use tao::event_loop::EventLoopProxy;
use tao::window::Window;
use wry::{
    NewWindowResponse, PageLoadEvent, PermissionKind, PermissionResponse, Rect, WebContext, WebView,
    WebViewBuilder, WebViewBuilderExtWindows,
};

use crate::app::{Responder, UserEvent};
use crate::content_ipc::{self, ContentMsg};
use crate::nav::{self, Nav};
use crate::protocol::{self, Route};
use crate::tabs::TabId;
use crate::webview2::{self, TabShared};
use crate::adblock;

/// Capture des identifiants : même script que le desktop, cadre principal.
const CAPTURE_JS: &str = include_str!("../../../assets/password_capture.js");

/// Arguments navigateur communs à TOUTES les WebView (même dossier de
/// données ⇒ arguments identiques obligatoires). wry désactive SmartScreen
/// par défaut : on le réactive en ne reprenant que ses autres retraits.
pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI";

/// Couleur de fond avant le premier rendu (`--nyx-deep`).
pub const BACKGROUND: (u8, u8, u8, u8) = (0x0a, 0x0a, 0x18, 0xff);

/// Construit la WebView de l'onglet `id` chargeant `url` (URL moteur).
pub fn build(
    window: &Window,
    context: &mut WebContext,
    proxy: &EventLoopProxy<UserEvent>,
    id: TabId,
    url: &str,
    bounds: Rect,
    visible: bool,
) -> wry::Result<WebView> {
    let shared = Rc::new(TabShared::default());

    let nav_shared = shared.clone();
    let nav_proxy = proxy.clone();
    let on_navigate = move |target: String| {
        let verdict = nav::decide_main(
            &target,
            &nav_shared.current.borrow(),
            &nav_shared.pending_main.borrow(),
            adblock::guard(),
        );
        match verdict {
            Nav::Allow => {
                *nav_shared.pending_main.borrow_mut() = target;
                true
            }
            Nav::Deny => false,
            Nav::Redirect(to) => {
                let _ = nav_proxy.send_event(UserEvent::Load { tab: id, url: to.into() });
                false
            }
            Nav::Action(action) => {
                let _ = nav_proxy.send_event(UserEvent::Action { tab: id, action });
                false
            }
        }
    };

    let popup_shared = shared.clone();
    let popup_proxy = proxy.clone();
    let on_new_window = move |target: String, _features| {
        // Jamais de fenêtre popup : nouvel onglet, après la même décision.
        // Une action interne n'ouvre jamais d'onglet.
        let url = match nav::decide(&target, &popup_shared.current.borrow(), adblock::guard()) {
            Nav::Allow => Some(target),
            Nav::Redirect(to) => Some(to.to_string()),
            Nav::Deny | Nav::Action(_) => None,
        };
        if let Some(url) = url {
            let _ = popup_proxy.send_event(UserEvent::OpenTab { url });
        }
        NewWindowResponse::Deny
    };

    let load_shared = shared.clone();
    let load_proxy = proxy.clone();
    let on_page_load = move |event: PageLoadEvent, url: String| {
        let loading = matches!(event, PageLoadEvent::Started);
        if loading {
            *load_shared.current.borrow_mut() = url;
        }
        let _ = load_proxy.send_event(UserEvent::PageLoad { tab: id, loading });
    };

    let title_proxy = proxy.clone();
    let on_title = move |title: String| {
        let _ = title_proxy.send_event(UserEvent::Title { tab: id, title });
    };

    // Canal natif des pages (capture, actions internes). `source` = URL du
    // document émetteur selon WebView2 (jamais déclarée par la page).
    let ipc_shared = shared.clone();
    let ipc_proxy = proxy.clone();
    let on_ipc = move |request: wry::http::Request<String>| {
        let source = request.uri().to_string();
        let event = match ContentMsg::parse(request.body()) {
            Some(ContentMsg::Password { username, password, host }) => {
                content_ipc::password_host(&source, &ipc_shared.current.borrow(), &host)
                    .map(|host| UserEvent::Credential { host, username, password })
            }
            Some(ContentMsg::Action { url }) => content_ipc::action(&source, &url, adblock::guard())
                .map(|action| UserEvent::Action { tab: id, action }),
            None => None,
        };
        if let Some(event) = event {
            let _ = ipc_proxy.send_event(event);
        }
    };

    // Pages internes : statiques servies tout de suite, celles qui lisent le
    // coffre rendues par l'app (thread UI). wry exige Send + Sync.
    let page_proxy = Mutex::new(proxy.clone());
    let on_protocol = move |_: wry::WebViewId<'_>, request: wry::http::Request<Vec<u8>>, responder: wry::RequestAsyncResponder| {
        let route = match protocol::target(&request) {
            Some((host, path)) => protocol::internal_route(&host, &path),
            None => Route::Static(protocol::method_not_allowed()),
        };
        match route {
            Route::Static(asset) => responder.respond(protocol::to_response(asset)),
            Route::Dynamic(page) => {
                if let Ok(proxy) = page_proxy.lock() {
                    let _ = proxy.send_event(UserEvent::Serve { page, responder: Responder(responder) });
                }
            }
        }
    };

    let view = WebViewBuilder::new_with_web_context(context)
        .with_url(url)
        .with_bounds(bounds)
        .with_visible(visible)
        .with_focused(visible)
        .with_background_color(BACKGROUND)
        .with_https_scheme(true)
        .with_additional_browser_args(BROWSER_ARGS)
        .with_theme(wry::Theme::Dark)
        .with_devtools(cfg!(debug_assertions))
        .with_hotkeys_zoom(true)
        .with_initialization_script_for_main_only(CAPTURE_JS, true)
        .with_asynchronous_custom_protocol("nyx".into(), on_protocol)
        .with_ipc_handler(on_ipc)
        .with_navigation_handler(on_navigate)
        .with_new_window_req_handler(on_new_window)
        .with_on_page_load_handler(on_page_load)
        .with_document_title_changed_handler(on_title)
        .with_permission_handler(permission)
        .build_as_child(window)?;

    if let Err(err) = webview2::install_content_hooks(&view, id, shared, proxy.clone()) {
        // Sans ces hooks l'onglet fonctionne, mais sans blocage : on le dit.
        nyx_core::sec_log::emit(nyx_core::sec_log::Level::Warn, &format!("hooks WebView2 : {err}"));
    }
    Ok(view)
}

/// Permissions : jamais d'accord automatique. Caméra, micro, position,
/// notifications, capture d'écran, presse-papiers… → invite native WebView2
/// (l'utilisateur décide) ; capteurs, MIDI, polices locales, gestion des
/// fenêtres → refus silencieux.
fn permission(kind: PermissionKind) -> PermissionResponse {
    match kind {
        PermissionKind::Midi
        | PermissionKind::Sensors
        | PermissionKind::LocalFonts
        | PermissionKind::WindowManagement => PermissionResponse::Deny,
        _ => PermissionResponse::Default,
    }
}
