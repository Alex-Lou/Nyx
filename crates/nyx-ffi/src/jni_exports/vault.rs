//! Exports JNI de `io.nyx.browser.NyxVault` (objet Kotlin, méthodes
//! `@JvmStatic external`). Marshalling uniquement : toute décision vit dans
//! [`crate::vault_api`]. Le coffre ouvert est une [`Session`] dans un
//! `Mutex` global (un seul coffre par application) — aucun pointeur ne
//! traverse vers Kotlin.
//!
//! Conventions : `String` vide = succès, sinon message d'erreur affichable
//! (jamais de clé dedans) ; listes en JSON ; identifiant `-1` = échec.
//! Les clés d'appareil arrivent en `ByteArray` : la copie Rust est effacée
//! ici, Kotlin efface la sienne après l'appel.
#![allow(unsafe_code)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jbyteArray, jint, jlong, jstring, JNI_FALSE};
use jni::JNIEnv;

use super::{new_string, read_string, to_jboolean};
use crate::vault_api::{self, Session};
use vault::keymode::KeyError;
use vault::Vault;

static SESSION: Mutex<Session> = Mutex::new(Session::new());

/// Verrou de la session (un verrou empoisonné reste utilisable : l'état du
/// coffre est dans SQLite, pas dans la structure).
fn session() -> MutexGuard<'static, Session> {
    SESSION.lock().unwrap_or_else(|p| p.into_inner())
}

/// Exécute `f` sans laisser une panique traverser la frontière FFI.
fn guarded<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// Copie Rust d'un `ByteArray` (à effacer par l'appelant).
fn read_bytes(env: &mut JNIEnv, bytes: &JByteArray) -> Vec<u8> {
    if bytes.is_null() {
        return Vec::new();
    }
    env.convert_byte_array(bytes).unwrap_or_default()
}

fn status(r: Result<(), KeyError>) -> String {
    r.err().map(|e| e.to_string()).unwrap_or_default()
}

fn status_str(r: Result<(), String>) -> String {
    r.err().unwrap_or_default()
}

/// Installe un coffre fraîchement ouvert et applique ses réglages.
fn install(r: Result<Vault, KeyError>) -> String {
    match r {
        Ok(v) => {
            let mut s = session();
            s.install(v);
            crate::set_app_settings(s.app_settings());
            String::new()
        }
        Err(e) => e.to_string(),
    }
}

fn ret_string(env: &mut JNIEnv, s: String) -> jstring {
    new_string(env, &s)
}

// ── Démarrage, ouverture, création ─────────────────────────────────────

/// `NyxVault.generateDeviceKey(): ByteArray?` — 32 octets du CSPRNG.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_generateDeviceKey<'l>(
    env: JNIEnv<'l>,
    _c: JClass<'l>,
) -> jbyteArray {
    let mut key = guarded(Vec::new(), || vault_api::generate_raw_key().unwrap_or_default());
    let out = if key.is_empty() {
        std::ptr::null_mut()
    } else {
        env.byte_array_from_slice(&key).map(|a| a.into_raw()).unwrap_or(std::ptr::null_mut())
    };
    vault_api::wipe(&mut key);
    out
}

/// `NyxVault.startup(dbPath: String, keyFileExists: Boolean): String` — `first|device|master`.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_startup<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    db: JString<'l>,
    keyfile_exists: jboolean,
) -> jstring {
    let db = PathBuf::from(read_string(&mut env, &db));
    let state = guarded("first", || vault_api::startup(&db, keyfile_exists != JNI_FALSE));
    new_string(&mut env, state)
}

/// `NyxVault.checkNewPassphrase(p: String, confirm: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_checkNewPassphrase<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    p: JString<'l>,
    confirm: JString<'l>,
) -> jstring {
    let (p, confirm) = (read_string(&mut env, &p), read_string(&mut env, &confirm));
    let msg = guarded("Opération impossible".to_string(), || vault_api::check_new_passphrase(&p, &confirm));
    ret_string(&mut env, msg)
}

/// `NyxVault.openWithDeviceKey(dbPath: String, key: ByteArray): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_openWithDeviceKey<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    db: JString<'l>,
    key: JByteArray<'l>,
) -> jstring {
    let db = PathBuf::from(read_string(&mut env, &db));
    let mut key = read_bytes(&mut env, &key);
    let msg = guarded("Opération impossible".to_string(), || install(vault_api::open_raw(&db, &mut key)));
    vault_api::wipe(&mut key);
    ret_string(&mut env, msg)
}

/// `NyxVault.openWithPassphrase(dbPath: String, passphrase: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_openWithPassphrase<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    db: JString<'l>,
    passphrase: JString<'l>,
) -> jstring {
    let db = PathBuf::from(read_string(&mut env, &db));
    let pass = vault::Passphrase::new(read_string(&mut env, &passphrase));
    let msg = guarded("Opération impossible".to_string(), || {
        install(vault_api::open_passphrase(&db, pass.expose()))
    });
    ret_string(&mut env, msg)
}

/// `NyxVault.createWithDeviceKey(dbPath: String, key: ByteArray): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_createWithDeviceKey<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    db: JString<'l>,
    key: JByteArray<'l>,
) -> jstring {
    let db = PathBuf::from(read_string(&mut env, &db));
    let mut key = read_bytes(&mut env, &key);
    let msg = guarded("Opération impossible".to_string(), || install(vault_api::create_raw(&db, &mut key)));
    vault_api::wipe(&mut key);
    ret_string(&mut env, msg)
}

/// `NyxVault.createWithPassphrase(dbPath: String, passphrase: String, confirm: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_createWithPassphrase<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    db: JString<'l>,
    passphrase: JString<'l>,
    confirm: JString<'l>,
) -> jstring {
    let db = PathBuf::from(read_string(&mut env, &db));
    let pass = vault::Passphrase::new(read_string(&mut env, &passphrase));
    let confirm = vault::Passphrase::new(read_string(&mut env, &confirm));
    let msg = guarded("Opération impossible".to_string(), || {
        install(vault_api::create_passphrase(&db, pass.expose(), confirm.expose()))
    });
    ret_string(&mut env, msg)
}

/// `NyxVault.close()`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_close<'l>(_env: JNIEnv<'l>, _c: JClass<'l>) {
    guarded((), || session().close());
}

/// `NyxVault.isOpen(): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_isOpen<'l>(_env: JNIEnv<'l>, _c: JClass<'l>) -> jboolean {
    to_jboolean(guarded(false, || session().is_open()))
}

/// `NyxVault.hasMasterPassword(): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_hasMasterPassword<'l>(
    _env: JNIEnv<'l>,
    _c: JClass<'l>,
) -> jboolean {
    to_jboolean(guarded(false, || session().has_master()))
}

// ── Mot de passe principal ─────────────────────────────────────────────

/// `NyxVault.verifyMaster(passphrase: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_verifyMaster<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    passphrase: JString<'l>,
) -> jstring {
    let pass = vault::Passphrase::new(read_string(&mut env, &passphrase));
    let msg = guarded("Opération impossible".to_string(), || status(session().verify_master(pass.expose())));
    ret_string(&mut env, msg)
}

/// `NyxVault.setMaster(new: String, confirm: String, currentKey: ByteArray): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_setMaster<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    new: JString<'l>,
    confirm: JString<'l>,
    current_key: JByteArray<'l>,
) -> jstring {
    let new = vault::Passphrase::new(read_string(&mut env, &new));
    let confirm = vault::Passphrase::new(read_string(&mut env, &confirm));
    let mut key = read_bytes(&mut env, &current_key);
    let msg = guarded("Opération impossible".to_string(), || {
        status(session().set_master(new.expose(), confirm.expose(), &mut key))
    });
    vault_api::wipe(&mut key);
    ret_string(&mut env, msg)
}

/// `NyxVault.changeMaster(old: String, new: String, confirm: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_changeMaster<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    old: JString<'l>,
    new: JString<'l>,
    confirm: JString<'l>,
) -> jstring {
    let old = vault::Passphrase::new(read_string(&mut env, &old));
    let new = vault::Passphrase::new(read_string(&mut env, &new));
    let confirm = vault::Passphrase::new(read_string(&mut env, &confirm));
    let msg = guarded("Opération impossible".to_string(), || {
        status(session().change_master(old.expose(), new.expose(), confirm.expose()))
    });
    ret_string(&mut env, msg)
}

/// `NyxVault.removeMaster(current: String, newKey: ByteArray): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_removeMaster<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    current: JString<'l>,
    new_key: JByteArray<'l>,
) -> jstring {
    let current = vault::Passphrase::new(read_string(&mut env, &current));
    let mut key = read_bytes(&mut env, &new_key);
    let msg = guarded("Opération impossible".to_string(), || {
        status(session().remove_master(current.expose(), &mut key))
    });
    vault_api::wipe(&mut key);
    ret_string(&mut env, msg)
}

// ── Mots de passe ──────────────────────────────────────────────────────

/// `NyxVault.passwordsJson(): String` — `[{id, domain, username}]`, sans mot de passe.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_passwordsJson<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
) -> jstring {
    let json = guarded(String::new(), || session().passwords_json().unwrap_or_else(|_| "[]".into()));
    ret_string(&mut env, json)
}

/// `NyxVault.passwordsForHost(host: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_passwordsForHost<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    host: JString<'l>,
) -> jstring {
    let host = read_string(&mut env, &host);
    let json = guarded(String::new(), || session().passwords_for_json(&host).unwrap_or_else(|_| "[]".into()));
    ret_string(&mut env, json)
}

/// `NyxVault.passwordDomainsJson(): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_passwordDomainsJson<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
) -> jstring {
    let json = guarded(String::new(), || session().password_domains_json().unwrap_or_else(|_| "[]".into()));
    ret_string(&mut env, json)
}

/// `NyxVault.passwordReveal(domain: String, id: Long): String?` — `null` si introuvable.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_passwordReveal<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    domain: JString<'l>,
    id: jlong,
) -> jstring {
    let domain = read_string(&mut env, &domain);
    match guarded(None, || session().password_reveal(&domain, id).ok()) {
        Some(pw) => {
            let pw = vault::Passphrase::new(pw);
            new_string(&mut env, pw.expose())
        }
        None => std::ptr::null_mut(),
    }
}

/// `NyxVault.passwordAdd(domain: String, username: String, password: String): Long`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_passwordAdd<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    domain: JString<'l>,
    username: JString<'l>,
    password: JString<'l>,
) -> jlong {
    let domain = read_string(&mut env, &domain);
    let username = read_string(&mut env, &username);
    let password = vault::Passphrase::new(read_string(&mut env, &password));
    guarded(-1, || session().password_add(&domain, &username, password.expose()).unwrap_or(-1))
}

/// `NyxVault.passwordUpdate(id: Long, password: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_passwordUpdate<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    id: jlong,
    password: JString<'l>,
) -> jboolean {
    let password = vault::Passphrase::new(read_string(&mut env, &password));
    to_jboolean(guarded(false, || session().password_update(id, password.expose()).is_ok()))
}

/// `NyxVault.passwordDelete(id: Long): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_passwordDelete<'l>(
    _env: JNIEnv<'l>,
    _c: JClass<'l>,
    id: jlong,
) -> jboolean {
    to_jboolean(guarded(false, || session().password_delete(id).is_ok()))
}

// ── Capture et pré-remplissage ─────────────────────────────────────────

/// `NyxVault.captureOffer(sourceOrigin: String, topUrl: String, message: String, private: Boolean): String`
/// — JSON `{host, question}` ou `""` (ignorer).
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_captureOffer<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    source_origin: JString<'l>,
    top_url: JString<'l>,
    message: JString<'l>,
    private: jboolean,
) -> jstring {
    let source = read_string(&mut env, &source_origin);
    let top = read_string(&mut env, &top_url);
    let message = vault::Passphrase::new(read_string(&mut env, &message));
    let json = guarded(String::new(), || {
        vault_api::capture_offer(&session(), &source, &top, message.expose(), private != JNI_FALSE)
            .and_then(|o| serde_json::to_string(&o).ok())
            .unwrap_or_default()
    });
    ret_string(&mut env, json)
}

/// `NyxVault.captureSave(host: String, message: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_captureSave<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    host: JString<'l>,
    message: JString<'l>,
) -> jboolean {
    let host = read_string(&mut env, &host);
    let message = vault::Passphrase::new(read_string(&mut env, &message));
    to_jboolean(guarded(false, || vault_api::capture_save(&session(), &host, message.expose()).unwrap_or(false)))
}

/// `NyxVault.autofillScript(pageUrl: String, private: Boolean): String` — `""` = rien.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_autofillScript<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    page_url: JString<'l>,
    private: jboolean,
) -> jstring {
    let url = read_string(&mut env, &page_url);
    let js = vault::Passphrase::new(guarded(String::new(), || session().autofill_script(&url, private != JNI_FALSE)));
    new_string(&mut env, js.expose())
}

// ── Favoris ────────────────────────────────────────────────────────────

/// `NyxVault.bookmarksJson(): String` — `[{id, url, title, folder}]`.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_bookmarksJson<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
) -> jstring {
    let json = guarded(String::new(), || session().bookmarks_json().unwrap_or_else(|_| "[]".into()));
    ret_string(&mut env, json)
}

/// `NyxVault.bookmarkFind(url: String): Long` — `-1` si absent.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_bookmarkFind<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    url: JString<'l>,
) -> jlong {
    let url = read_string(&mut env, &url);
    guarded(-1, || session().bookmark_find(&url).ok().flatten().unwrap_or(-1))
}

/// `NyxVault.bookmarkAdd(url: String, title: String): Long`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_bookmarkAdd<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    url: JString<'l>,
    title: JString<'l>,
) -> jlong {
    let url = read_string(&mut env, &url);
    let title = read_string(&mut env, &title);
    guarded(-1, || session().bookmark_add(&url, &title).unwrap_or(-1))
}

/// `NyxVault.bookmarkDelete(id: Long): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_bookmarkDelete<'l>(
    _env: JNIEnv<'l>,
    _c: JClass<'l>,
    id: jlong,
) -> jboolean {
    to_jboolean(guarded(false, || session().bookmark_delete(id).is_ok()))
}

// ── Historique ─────────────────────────────────────────────────────────

/// `NyxVault.historyPush(url: String, title: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_historyPush<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    url: JString<'l>,
    title: JString<'l>,
) -> jboolean {
    let url = read_string(&mut env, &url);
    let title = read_string(&mut env, &title);
    to_jboolean(guarded(false, || session().history_push(&url, &title).unwrap_or(false)))
}

/// `NyxVault.historyRetitle(url: String, title: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_historyRetitle<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    url: JString<'l>,
    title: JString<'l>,
) -> jboolean {
    let url = read_string(&mut env, &url);
    let title = read_string(&mut env, &title);
    to_jboolean(guarded(false, || session().history_retitle(&url, &title).unwrap_or(false)))
}

/// `NyxVault.historyRecentJson(limit: Int): String` — `[{id, url, title, visited_at}]`.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_historyRecentJson<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    limit: jint,
) -> jstring {
    let limit = usize::try_from(limit).unwrap_or(vault_api::HISTORY_SHOWN);
    let json = guarded(String::new(), || session().history_recent_json(limit).unwrap_or_else(|_| "[]".into()));
    ret_string(&mut env, json)
}

/// `NyxVault.historySearchJson(query: String): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_historySearchJson<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    query: JString<'l>,
) -> jstring {
    let query = read_string(&mut env, &query);
    let json = guarded(String::new(), || session().history_search_json(&query).unwrap_or_else(|_| "[]".into()));
    ret_string(&mut env, json)
}

/// `NyxVault.historyClear(): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_historyClear<'l>(_env: JNIEnv<'l>, _c: JClass<'l>) -> jboolean {
    to_jboolean(guarded(false, || session().history_clear().is_ok()))
}

// ── Réglages ───────────────────────────────────────────────────────────

/// `NyxVault.settingGet(key: String): String?` — `null` si absent.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_settingGet<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    key: JString<'l>,
) -> jstring {
    let key = read_string(&mut env, &key);
    match guarded(None, || session().setting(&key).ok().flatten()) {
        Some(v) => new_string(&mut env, &v),
        None => std::ptr::null_mut(),
    }
}

/// `NyxVault.settingSet(key: String, value: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_settingSet<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    key: JString<'l>,
    value: JString<'l>,
) -> jboolean {
    let key = read_string(&mut env, &key);
    let value = read_string(&mut env, &value);
    to_jboolean(guarded(false, || session().set_setting(&key, &value).is_ok()))
}

/// `NyxVault.settingsJson(): String` — `{engine, home, adblock, savepw, master}`.
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_settingsJson<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>) -> jstring {
    let json = guarded(String::new(), || session().settings_json());
    ret_string(&mut env, json)
}

/// `NyxVault.settingsUpdate(engine: String, home: String, adblock: Boolean, savePasswords: Boolean): String`
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_settingsUpdate<'l>(
    mut env: JNIEnv<'l>,
    _c: JClass<'l>,
    engine: JString<'l>,
    home: JString<'l>,
    adblock: jboolean,
    save_passwords: jboolean,
) -> jstring {
    let engine = read_string(&mut env, &engine);
    let home = read_string(&mut env, &home);
    let msg = guarded("Opération impossible".to_string(), || {
        let r = session().update_app_settings(&engine, &home, adblock != JNI_FALSE, save_passwords != JNI_FALSE);
        status_str(r.map(crate::set_app_settings))
    });
    ret_string(&mut env, msg)
}

/// `NyxVault.homeUrl(): String` — page d'accueil courante (`nyx://newtab` par défaut).
#[no_mangle]
pub extern "system" fn Java_io_nyx_browser_NyxVault_homeUrl<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>) -> jstring {
    let home = guarded(String::new(), || crate::app_settings().home_url);
    ret_string(&mut env, home)
}
