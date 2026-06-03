//! Bridge WebKit → DownloadStore : wire les signaux d'un `WebContext`.
//!
//! Pipeline (cf. plan §3) :
//!   1. `download-started` → pre-flight `download_policy::analyze`
//!      → Block ⇒ cancel ; sinon allocate temp path + add au store.
//!   2. `decide-destination` → on impose le temp path Nyx.
//!   3. `received-data` → update progress (throttle 200 ms).
//!   4. `finished` → post-flight : re-sniff + sha256 + verdict adjusté
//!      → Block ⇒ delete + mark_failed
//!      → Allow ⇒ move + meta + mark_completed
//!      → Ask/AskDanger ⇒ dialog, branche selon réponse user.
//!   5. `failed` → delete temp immédiat + mark_failed.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use gtk::prelude::*;
use webkit2gtk::{
    Download, DownloadExt, URIRequestExt, URIResponseExt, WebContext, WebContextExt,
};

use nyx_core::download_policy::{self, Kind, Mode, Request, Verdict};
use nyx_core::downloads::{
    self as core_dl, quarantine, DownloadId,
};
use nyx_core::magic_bytes;
use nyx_core::sec_log::{self, Level};

use crate::state::downloads::DownloadsHandle;
use crate::state::settings::Settings;

use super::{cleanup, confirm, io as dio};

const PROGRESS_THROTTLE_MS: u128 = 200;

pub fn wire(
    ctx: &WebContext, store: DownloadsHandle, settings: Settings, temp_root: PathBuf,
) {
    // Boot scan : nettoie les fichiers > 24h du run précédent.
    let _ = crate::state::downloads_temp::ensure(&temp_root);
    cleanup::scan_and_purge(&temp_root, now_unix());

    ctx.connect_download_started({
        let store = store.clone();
        let settings = settings.clone();
        let temp_root = temp_root.clone();
        move |_ctx, download| {
            on_started(download, store.clone(), settings.clone(), temp_root.clone());
        }
    });
}

// ─── Démarrage ─────────────────────────────────────────────────────────────

fn on_started(
    download: &Download, store: DownloadsHandle, settings: Settings, temp_root: PathBuf,
) {
    let req = read_request(download);
    let mode = current_mode(&settings);
    let pre = download_policy::analyze(&Request {
        suggested_filename: &req.suggested,
        mimetype:           &req.mime,
        initiator_url:      &req.uri,
        final_url:          &req.uri,
        content_length:     req.content_length,
        mode,
    });

    if pre.verdict == Verdict::Block {
        sec_log::emit(Level::Block, &format!(
            "download blocked pre-flight: {} reasons={}",
            sec_log::redact_url(&req.uri), pre.reasons.len(),
        ));
        download.cancel();
        return;
    }

    let random = core_dl::fresh_random_bytes();
    let temp_path = core_dl::build_temp_path(&temp_root, &random, &pre.normalized_filename);

    let started_at = now_unix();
    let id = store.borrow_mut().add(
        pre.normalized_filename.clone(),
        temp_path.to_string_lossy().into_owned(),
        pre.kind,
        pre.final_origin.clone(),
        started_at,
    );

    let pre_rc = Rc::new(pre);
    let temp_rc = Rc::new(temp_path);
    let req_rc = Rc::new(req);

    wire_decide_destination(download, temp_rc.clone());
    wire_progress(download, store.clone(), id);
    wire_finished(download, store.clone(), settings.clone(),
                  id, pre_rc, temp_rc.clone(), req_rc, started_at);
    wire_failed(download, store, id, temp_rc);
}

// ─── Signaux par téléchargement ────────────────────────────────────────────

fn wire_decide_destination(download: &Download, temp: Rc<PathBuf>) {
    download.connect_decide_destination(move |dl, _suggested| {
        dl.set_destination(&format!("file://{}", temp.to_string_lossy()));
        true
    });
}

fn wire_progress(download: &Download, store: DownloadsHandle, id: DownloadId) {
    let last = Rc::new(RefCell::new(Instant::now()));
    download.connect_received_data(move |dl, _len| {
        let mut tick = last.borrow_mut();
        if tick.elapsed().as_millis() < PROGRESS_THROTTLE_MS { return; }
        *tick = Instant::now();
        drop(tick);
        let received = dl.received_data_length();
        let total = dl.response().map(|r| r.content_length()).filter(|&n| n > 0);
        store.borrow_mut().update_progress(id, received, total);
    });
}

fn wire_failed(
    download: &Download, store: DownloadsHandle, id: DownloadId, temp: Rc<PathBuf>,
) {
    download.connect_failed(move |_dl, _err| {
        // Defense-in-depth invariant §17 : delete temp immédiat.
        let _ = std::fs::remove_file(temp.as_path());
        store.borrow_mut().mark_failed(id, "Échec du téléchargement".into(), now_unix());
    });
}

fn wire_finished(
    download: &Download, store: DownloadsHandle, settings: Settings, id: DownloadId,
    pre: Rc<download_policy::Report>, temp: Rc<PathBuf>, req: Rc<RequestData>,
    started_at: i64,
) {
    download.connect_finished(move |_dl| {
        on_finished(
            store.clone(), settings.clone(), id,
            pre.clone(), temp.clone(), req.clone(), started_at,
        );
    });
}

// ─── Post-flight ───────────────────────────────────────────────────────────

fn on_finished(
    store: DownloadsHandle, settings: Settings, id: DownloadId,
    pre: Rc<download_policy::Report>, temp: Rc<PathBuf>, req: Rc<RequestData>,
    started_at: i64,
) {
    let head = match dio::read_first_8k(&temp) {
        Ok(b) => b,
        Err(_) => {
            sec_log::emit(Level::Warn, &format!(
                "download finished but temp unreadable: {}", pre.normalized_filename));
            let _ = std::fs::remove_file(temp.as_path());
            store.borrow_mut().mark_failed(id, "Fichier temporaire illisible".into(), now_unix());
            return;
        }
    };
    let sniffed = magic_bytes::sniff(&head);
    let live_kind = magic_bytes::category(sniffed);
    let normalized_ext = pre.normalized_extension.as_deref().unwrap_or("");
    let ext_consistent = normalized_ext.is_empty()
        || matches!(sniffed, magic_bytes::Detected::Unknown)
        || magic_bytes::matches_extension(sniffed, normalized_ext);

    let sha256 = dio::sha256_file(&temp).unwrap_or_default();
    let size = std::fs::metadata(temp.as_path()).map(|m| m.len()).unwrap_or(0);

    let final_verdict = adjust_verdict(pre.verdict, pre.kind, live_kind, ext_consistent);
    sec_log::emit(Level::Warn, &format!(
        "download post-flight: {} pre={:?} sniff={:?} ext_ok={} → {:?}",
        pre.normalized_filename, pre.verdict, sniffed, ext_consistent, final_verdict,
    ));

    match final_verdict {
        Verdict::Block => {
            let _ = std::fs::remove_file(temp.as_path());
            store.borrow_mut().mark_failed(id, "Bloqué par DownloadGuard".into(), now_unix());
        }
        Verdict::Allow => commit(
            store, settings, id, &pre, &temp, &req,
            started_at, sha256, size, sniffed, live_kind,
        ),
        Verdict::Ask | Verdict::AskDanger => {
            // Capture everything by Rc/Clone for the user callback (lifetime
            // separate from the dialog's GTK signal closure).
            let store_cb = store.clone();
            let settings_cb = settings.clone();
            let pre_cb = pre.clone();
            let temp_cb = temp.clone();
            let req_cb = req.clone();
            confirm::show(
                final_verdict, pre.normalized_filename.clone(), live_kind,
                move |approved| {
                    if approved {
                        commit(
                            store_cb, settings_cb, id, &pre_cb, &temp_cb, &req_cb,
                            started_at, sha256, size, sniffed, live_kind,
                        );
                    } else {
                        let _ = std::fs::remove_file(temp_cb.as_path());
                        store_cb.borrow_mut().mark_cancelled(id, now_unix());
                    }
                },
            );
        }
    }
}

/// Si le sniff post-flight escalate le danger par rapport au pre-flight,
/// on **élève** le verdict (jamais l'inverse). MIME-mismatch sévère →
/// refus sec.
fn adjust_verdict(pre: Verdict, pre_kind: Kind, live: Kind, ext_consistent: bool) -> Verdict {
    if !ext_consistent && matches!(live, Kind::Executable | Kind::MacroDoc) {
        return Verdict::Block;
    }
    let live_dangerous = matches!(live, Kind::Executable | Kind::MacroDoc);
    let pre_safe = matches!(pre_kind, Kind::Safe);
    if live_dangerous && pre_safe { return Verdict::Block; }
    pre
}

#[allow(clippy::too_many_arguments)]
fn commit(
    store: DownloadsHandle, settings: Settings, id: DownloadId,
    pre: &download_policy::Report, temp: &Path, req: &RequestData,
    started_at: i64, sha256: String, size: u64,
    sniffed: magic_bytes::Detected, live_kind: Kind,
) {
    let final_dir = settings.borrow().downloads_dir.clone()
        .map(PathBuf::from)
        .or_else(crate::platform::default_downloads_dir);
    let Some(final_dir) = final_dir else {
        let _ = std::fs::remove_file(temp);
        store.borrow_mut().mark_failed(id, "Dossier de téléchargement introuvable".into(), now_unix());
        return;
    };

    let finished_at = now_unix();
    let meta = quarantine::QuarantineMeta {
        schema:           quarantine::SCHEMA_VERSION,
        sha256,
        size_bytes:       size,
        sniffed:          format!("{sniffed:?}"),
        declared_ext:     pre.normalized_extension.clone().unwrap_or_default(),
        declared_mime:    req.mime.clone(),
        source_host:      host_only(&pre.initiator_origin),
        final_host:       host_only(&pre.final_origin),
        started_at_unix:  started_at,
        finished_at_unix: finished_at,
        verdict:          format!("{:?}", pre.verdict),
        reasons:          pre.reasons.iter().map(|r| format!("{r:?}").chars()
                            .filter(|c| c.is_ascii_alphabetic()).collect::<String>())
                            .filter(|s| !s.is_empty()).collect(),
    };
    let meta_content = match quarantine::serialize(&meta) {
        Ok(s) => s,
        Err(e) => {
            sec_log::emit(Level::Warn,
                &format!("meta serialize failed: {e:?} — proceeding without sidecar"));
            String::new()
        }
    };

    match dio::move_with_meta(temp, &final_dir, &pre.normalized_filename, &meta_content) {
        Ok(final_path) => {
            let _ = live_kind; // logged via post-flight already
            store.borrow_mut().mark_completed(
                id, Some(final_path.to_string_lossy().into_owned()), finished_at,
            );
        }
        Err(e) => {
            sec_log::emit(Level::Warn, &format!(
                "download move failed: {} ({e})", pre.normalized_filename));
            let _ = std::fs::remove_file(temp);
            store.borrow_mut().mark_failed(id, "Déplacement final impossible".into(), now_unix());
        }
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

struct RequestData {
    uri:            String,
    suggested:      String,
    mime:           String,
    content_length: Option<u64>,
}

fn read_request(download: &Download) -> RequestData {
    let uri = download.request().and_then(|r| r.uri()).map(|u| u.to_string()).unwrap_or_default();
    let suggested = download.suggested_filename()
        .map(|s| s.to_string()).unwrap_or_default();
    let response = download.response();
    let mime = response.as_ref().and_then(|r| r.mime_type()).map(|m| m.to_string()).unwrap_or_default();
    let content_length = response.map(|r| r.content_length()).filter(|&n| n > 0);
    RequestData { uri, suggested, mime, content_length }
}

fn current_mode(settings: &Settings) -> Mode {
    let s = settings.borrow();
    if s.private_mode { Mode::Shadow } else { Mode::Normal }
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Garde uniquement `host` d'une origin canonique `scheme://host`.
/// Renvoie `""` pour les origins vides ou non-http(s).
fn host_only(origin: &str) -> String {
    origin.split_once("://")
        .map(|(_, host)| host.to_string())
        .unwrap_or_default()
}
