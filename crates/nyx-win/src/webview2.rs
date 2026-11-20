//! Seul module `unsafe` de nyx-win : accès COM WebView2 pour ce que wry
//! n'expose pas.
//!
//! • filtre `WebResourceRequested` (blocage pub/traqueurs des sous-ressources) ;
//! • prévention du pistage `Strict` (API Profile) ;
//! • `DownloadStarting` SANS masquer l'UI native (SmartScreen, MOTW) ;
//! • `HistoryChanged` / `SourceChanged`, précédent/suivant/arrêt, titre ;
//! • raccourcis clavier (`AcceleratorKeyPressed`) ;
//! • boîte d'erreur fatale.
//!
//! SÛRETÉ : chaque appel COM porte sur une interface obtenue de wry (ou d'un
//! `cast` vérifié) et vivante pendant l'appel ; les chaînes sorties (`PWSTR`)
//! sont libérées par `take_pwstr`. Les callbacks s'exécutent sur le thread UI
//! (modèle de threading WebView2), comme le reste de la coquille. Aucune
//! politique ici : les décisions viennent de `adblock`, `downloads`, `keys`.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use tao::event_loop::EventLoopProxy;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler, DownloadStartingEventHandler,
    HistoryChangedEventHandler, SourceChangedEventHandler, WebResourceRequestedEventHandler,
};
use windows::core::{w, Interface, BOOL, HSTRING, PWSTR};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
use wry::{WebView, WebViewExtWindows};

use crate::app::UserEvent;
use crate::downloads::{self, Plan};
use crate::tabs::TabId;
use crate::{adblock, keys};

/// État d'un onglet partagé entre les handlers wry et COM (thread UI).
#[derive(Debug, Default)]
pub struct TabShared {
    /// URL de la page engagée (source des requêtes, frontière interne).
    pub current: RefCell<String>,
    /// URL de la navigation principale en cours (seul « document » jamais bloqué).
    pub pending_main: RefCell<String>,
}

/// Branche les hooks COM d'un onglet de contenu.
pub fn install_content_hooks(
    view: &WebView,
    tab: TabId,
    shared: Rc<TabShared>,
    proxy: EventLoopProxy<UserEvent>,
) -> windows::core::Result<()> {
    let core = view.webview();
    let env = view.environment();
    set_strict_tracking_prevention(&core);
    let mut token = 0i64;
    unsafe {
        // ── Blocage des sous-ressources ──────────────────────────────────
        match core.cast::<ICoreWebView2_22>() {
            Ok(core22) => core22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                w!("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )?,
            Err(_) => core.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?,
        }
        let blocker = shared.clone();
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                args.Request()?.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                args.ResourceContext(&mut context)?;
                let kind = resource_type(context, &uri, &blocker.pending_main.borrow());
                if adblock::should_block(&uri, &blocker.current.borrow(), kind) {
                    let response = env.CreateWebResourceResponse(None, 403, w!("Blocked"), w!(""))?;
                    args.SetResponse(&response)?;
                }
                Ok(())
            })),
            &mut token,
        )?;

        // ── Téléchargements : nom assaini, UI native conservée ───────────
        let dl_proxy = proxy.clone();
        core.cast::<ICoreWebView2_4>()?.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let op = args.DownloadOperation()?;
                let (mut uri, mut mime, mut path) = (PWSTR::null(), PWSTR::null(), PWSTR::null());
                op.Uri(&mut uri)?;
                op.MimeType(&mut mime)?;
                args.ResultFilePath(&mut path)?;
                let (uri, mime, path) = (take_pwstr(uri), take_pwstr(mime), PathBuf::from(take_pwstr(path)));
                let fallback = std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Downloads"));
                let event = match downloads::plan(&path, &uri, &mime, fallback.as_deref(), Path::exists) {
                    Plan::Save { name, path } => {
                        if let Some(path) = path {
                            args.SetResultFilePath(&HSTRING::from(path.as_os_str()))?;
                        }
                        UserEvent::Download { name, blocked: false }
                    }
                    Plan::Cancel { name } => {
                        args.SetCancel(true)?;
                        UserEvent::Download { name, blocked: true }
                    }
                };
                let _ = dl_proxy.send_event(event);
                Ok(())
            })),
            &mut token,
        )?;

        // ── URL / historique (y compris pushState des SPA) ───────────────
        let history_proxy = proxy.clone();
        core.add_HistoryChanged(
            &HistoryChangedEventHandler::create(Box::new(move |_, _| {
                let _ = history_proxy.send_event(UserEvent::History { tab });
                Ok(())
            })),
            &mut token,
        )?;
        let source_proxy = proxy.clone();
        core.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |_, _| {
                let _ = source_proxy.send_event(UserEvent::History { tab });
                Ok(())
            })),
            &mut token,
        )?;
    }
    install_shortcuts(view, proxy)
}

/// Type adblock d'une requête. Un document qui n'est pas la navigation
/// principale en cours est une (i)frame : `subdocument`, donc filtrable.
fn resource_type(context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT, uri: &str, pending_main: &str) -> &'static str {
    match context {
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => adblock::document_type(uri, pending_main),
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => {
            "xmlhttprequest"
        }
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
        _ => "other",
    }
}

/// Prévention du pistage WebView2 au niveau `Strict` (profil partagé).
/// Best effort : un runtime trop ancien ignore simplement le réglage.
fn set_strict_tracking_prevention(core: &ICoreWebView2) {
    let _ = unsafe {
        core.cast::<ICoreWebView2_13>()
            .and_then(|c| c.Profile())
            .and_then(|p| p.cast::<ICoreWebView2Profile3>())
            .and_then(|p| p.SetPreferredTrackingPreventionLevel(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT))
    };
}

/// Raccourcis globaux (chrome et contenu) → `UserEvent::Shortcut`.
pub fn install_shortcuts(view: &WebView, proxy: EventLoopProxy<UserEvent>) -> windows::core::Result<()> {
    let controller = view.controller();
    let mut token = 0i64;
    unsafe {
        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                args.KeyEventKind(&mut kind)?;
                if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN {
                    return Ok(());
                }
                let mut vk = 0u32;
                args.VirtualKey(&mut vk)?;
                let ctrl = GetKeyState(i32::from(VK_CONTROL.0)) < 0;
                let shift = GetKeyState(i32::from(VK_SHIFT.0)) < 0;
                if let Some(shortcut) = keys::shortcut(vk, ctrl, shift) {
                    args.SetHandled(true)?;
                    let _ = proxy.send_event(UserEvent::Shortcut(shortcut));
                }
                Ok(())
            })),
            &mut token,
        )
    }
}

/// `(peut reculer, peut avancer)`.
pub fn history_state(view: &WebView) -> (bool, bool) {
    let core = view.webview();
    let (mut back, mut forward) = (BOOL::default(), BOOL::default());
    unsafe {
        let _ = core.CanGoBack(&mut back);
        let _ = core.CanGoForward(&mut forward);
    }
    (back.as_bool(), forward.as_bool())
}

/// Titre courant du document principal (historique à la fin du chargement).
pub fn document_title(view: &WebView) -> String {
    let mut title = PWSTR::null();
    match unsafe { view.webview().DocumentTitle(&mut title) } {
        Ok(()) => take_pwstr(title),
        Err(_) => String::new(),
    }
}

pub fn go_back(view: &WebView) {
    let _ = unsafe { view.webview().GoBack() };
}

pub fn go_forward(view: &WebView) {
    let _ = unsafe { view.webview().GoForward() };
}

pub fn stop(view: &WebView) {
    let _ = unsafe { view.webview().Stop() };
}

/// Ouvre le panneau de téléchargements natif de WebView2.
pub fn open_downloads(view: &WebView) {
    let _ = unsafe {
        view.webview()
            .cast::<ICoreWebView2_9>()
            .and_then(|c| c.OpenDefaultDownloadDialog())
    };
}

/// Boîte d'erreur modale (le binaire n'a pas de console).
pub fn error_box(text: &str) {
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), w!("Nyx"), MB_OK | MB_ICONERROR);
    }
}
