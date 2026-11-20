//! Exports JNI de `io.nyx.browser.NyxSync` (appareils liés). Marshalling
//! uniquement : orchestration dans [`crate::sync_api`], protocole dans
//! `nyx-sync`. Les appels marqués « bloquant » doivent être faits depuis un
//! fil d'arrière-plan Kotlin ; le verrou de session n'est jamais tenu
//! pendant une opération réseau.
//!
//! Conventions : `String` vide = succès, sinon message affichable ; états
//! et listes en JSON.
#![allow(unsafe_code)]

use std::time::Duration;

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jstring, JNI_FALSE};
use jni::JNIEnv;

use super::vault::session;
use super::{new_string, read_string};
use crate::sync_api::{self, PairState};

/// Attente du code à 6 chiffres (connexion + handshake ; l'hôte peut être
/// lent à répondre si le pare-feu hésite).
const WAIT_CODE: Duration = Duration::from_secs(90);
/// Attente de la fin après la réponse (le PC confirme de son côté).
const WAIT_DONE: Duration = Duration::from_secs(150);

const LOCKED: &str = "Coffre verrouillé";

fn guarded<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(fallback)
}

fn error_json(message: &str) -> String {
    PairState::Done(Err(message.to_string())).to_json()
}

/// `NyxSync.pairBegin(offer: String, deviceName: String): String` —
/// bloquant. `{"state":"confirm","code","pc"}` ou `{"state":"error",…}`.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxSync_pairBegin<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    offer: JString<'l>,
    name: JString<'l>,
) -> jstring {
    let (offer, name) = (read_string(&mut env, &offer), read_string(&mut env, &name));
    let json = guarded(error_json("Opération impossible"), || {
        let Some((db, key)) = session().sync_target() else { return error_json(LOCKED) };
        sync_api::begin_pairing(db, key, offer, name).wait_code(WAIT_CODE).to_json()
    });
    new_string(&mut env, &json)
}

/// `NyxSync.pairConfirm(yes: Boolean): String` — bloquant.
/// `{"state":"done","pc"}` ou `{"state":"error",…}`.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxSync_pairConfirm<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    yes: jboolean,
) -> jstring {
    let json = guarded(error_json("Opération impossible"), || match sync_api::current_pairing() {
        Some(p) => p.answer(yes != JNI_FALSE, WAIT_DONE).to_json(),
        None => error_json("Aucun appairage en cours"),
    });
    new_string(&mut env, &json)
}

/// `NyxSync.pairCancel()` — non bloquant.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxSync_pairCancel<'l>(_env: JNIEnv<'l>, _c: JClass<'l>) {
    guarded((), || {
        if let Some(p) = sync_api::current_pairing() {
            p.cancel();
        }
    });
}

/// `NyxSync.syncNow(): String` — bloquant (découverte ~3 s + sessions).
/// `{"at_ms","ok","message","changed"}`. Données reçues → réglages
/// réappliqués au bloqueur et à la barre d'adresse.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxSync_syncNow<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>) -> jstring {
    let json = guarded(String::new(), || {
        let Some((db, key)) = session().sync_target() else {
            return sync_api::LastSync { message: LOCKED.into(), ..Default::default() }.to_json();
        };
        let last = sync_api::sync_now(&db, &key, sync_api::BROWSE);
        if last.changed {
            let s = session();
            if s.is_open() {
                crate::set_app_settings(s.app_settings());
            }
        }
        last.to_json()
    });
    new_string(&mut env, &json)
}

/// `NyxSync.devicesJson(): String` — `[{id,name,addedAt,revoked,me}]`.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxSync_devicesJson<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>) -> jstring {
    let json = guarded("[]".to_string(), || {
        session().open_vault().map(sync_api::devices_json).unwrap_or_else(|| "[]".into())
    });
    new_string(&mut env, &json)
}

/// `NyxSync.setDeviceName(name: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxSync_setDeviceName<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    name: JString<'l>,
) -> jstring {
    let name = read_string(&mut env, &name);
    let msg = guarded("Opération impossible".to_string(), || match session().open_vault() {
        Some(v) => sync_api::set_device_name(v, &name).err().unwrap_or_default(),
        None => LOCKED.into(),
    });
    new_string(&mut env, &msg)
}

/// `NyxSync.revoke(deviceId: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxSync_revoke<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    id: JString<'l>,
) -> jstring {
    let id = read_string(&mut env, &id);
    let msg = guarded("Opération impossible".to_string(), || match session().open_vault() {
        Some(v) => sync_api::revoke(v, &id).err().unwrap_or_default(),
        None => LOCKED.into(),
    });
    new_string(&mut env, &msg)
}

/// `NyxSync.lastSync(): String` — JSON de la dernière synchro, `""` si aucune.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxSync_lastSync<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>) -> jstring {
    let json = guarded(String::new(), || session().open_vault().map(sync_api::last_sync_json).unwrap_or_default());
    new_string(&mut env, &json)
}
