//! nyx-ffi — pont mince entre `nyx-core` et l'app Android.
//!
//! Deux couches :
//!   • ce fichier : helpers Rust **purs** (testés sur l'hôte), qui ne font
//!     qu'appeler `nyx-core` et tenir l'état global thread-safe ;
//!   • `jni_exports` : marshalling JNI (`Java_io_nyx_browser_NyxCore_*`),
//!     aucune logique.
//!
//! Thread-safety : `WebViewClient.shouldInterceptRequest` tourne sur les
//! threads réseau de la WebView. L'état global est donc en `OnceLock`
//! (`AdFilter` et `NyxGuard` sont `Send + Sync`), lu sans verrou.
//!
//! Coffre : module [`vault_api`] (helpers purs) + `jni_exports::vault`
//! (`Java_io_nyx_browser_NyxVault_*`), même modèle. Les réglages lus dans
//! le coffre pilotent ici la barre d'adresse et le bloqueur.
//!
//! Appareils liés : module [`sync_api`] (appairage en deux temps,
//! synchronisation sur connexion dédiée) + `jni_exports::sync`
//! (`Java_io_nyx_browser_NyxSync_*`) ; le protocole vit dans `nyx-sync`.

use std::sync::{OnceLock, RwLock};

use nyx_core::adfilter::{self, AdFilter};
use nyx_core::download_policy;
use nyx_core::nyxguard::NyxGuard;
use nyx_core::state::settings::AppSettings;
use nyx_core::{theme, url};

mod jni_exports;
pub mod sync_api;
pub mod vault_api;

static FILTER: OnceLock<AdFilter> = OnceLock::new();
static GUARD: OnceLock<NyxGuard> = OnceLock::new();
/// Réglages courants (coffre ouvert) ; `None` = défauts.
static SETTINGS: RwLock<Option<AppSettings>> = RwLock::new(None);

/// Réglages courants (copie).
pub fn app_settings() -> AppSettings {
    SETTINGS.read().ok().and_then(|g| g.clone()).unwrap_or_default()
}

/// Installe les réglages lus dans le coffre : barre d'adresse (moteur) et
/// bloqueur (NyxGuard + EasyList suivent le même interrupteur).
pub fn set_app_settings(s: AppSettings) {
    guard().set_enabled(s.adblock_enabled);
    guard().set_block_accounts(s.block_third_party);
    if let Ok(mut g) = SETTINGS.write() {
        *g = Some(s);
    }
}

fn guard() -> &'static NyxGuard {
    GUARD.get_or_init(NyxGuard::new)
}

/// Construit le moteur EasyList/EasyPrivacy depuis des listes gzippées.
/// Idempotent : un second appel ne reconstruit rien. Coûteux (~1 s sur un
/// téléphone) — à appeler hors du thread UI. Avant la fin de l'init, seul
/// le jeu de règles intégré de `NyxGuard` s'applique.
pub fn init_filters(gz_lists: &[Vec<u8>]) -> bool {
    FILTER.get_or_init(|| AdFilter::from_gz_lists(gz_lists.iter().map(Vec::as_slice)));
    true
}

/// `true` une fois le moteur EasyList prêt.
pub fn filters_ready() -> bool {
    FILTER.get().is_some()
}

/// Décision de blocage d'une sous-ressource. Ne bloque JAMAIS un document
/// principal (`resource_type == "document"`) ni un scheme non-http(s).
pub fn should_block(url: &str, source_url: &str, resource_type: &str) -> bool {
    if resource_type == "document" {
        return false;
    }
    if !matches!(url::scheme_of(url).as_deref(), Some("http" | "https")) {
        return false;
    }
    if guard().should_block(url) {
        return true;
    }
    guard().is_enabled()
        && FILTER
            .get()
            .is_some_and(|f| f.should_block(url, source_url, resource_type))
}

/// Type adblock d'une requête dont la WebView ne donne que l'en-tête Accept.
pub fn resource_type(url: &str, accept: &str) -> &'static str {
    adfilter::infer_resource_type(url, accept)
}

/// Barre d'adresse → URL (ou recherche avec le moteur choisi, DuckDuckGo
/// par défaut).
/// Renvoie `""` si le résultat n'est pas navigable (ex. `file://…`, refusé
/// sur Android). `nyx://…` est conservé : l'app le mappe sur la page
/// Nouvel onglet.
pub fn resolve_input(text: &str) -> String {
    let resolved = url::resolve_input(text, &app_settings());
    if url::is_navigable(&resolved) { resolved } else { String::new() }
}

/// `http`, `https` ou `nyx` uniquement.
pub fn is_navigable(target: &str) -> bool {
    url::is_navigable(target)
}

/// Substitue la palette Nyx dans une page interne (`{{TOKENS}}`).
pub fn render_page(template: &str) -> String {
    theme::render_page(template)
}

/// Nom de fichier sûr pour un téléchargement (sanitizer de
/// `download_policy`) : jamais vide, sans séparateur, NUL, contrôle ni bidi,
/// ≤ 255 octets, ni `.`/`..`, noms réservés Windows préfixés.
pub fn sanitize_filename(raw: &str) -> String {
    download_policy::analyze(&download_policy::Request::direct(raw, "", "")).normalized_filename
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve() {
        assert_eq!(resolve_input("github.com"), "https://github.com");
        assert!(resolve_input("rust async").starts_with("https://duckduckgo.com/"));
        assert_eq!(resolve_input("nyx://newtab"), "nyx://newtab");
        assert_eq!(resolve_input("file:///etc/passwd"), "");
        assert_eq!(resolve_input("   "), "");
        assert!(resolve_input("javascript:alert(1)").starts_with("https://duckduckgo.com/"));

        // Réglages du coffre : le moteur choisi suit (bloqueur laissé actif,
        // d'autres tests en dépendent).
        set_app_settings(AppSettings {
            search_engine: nyx_core::state::settings::SearchEngine::Brave,
            ..AppSettings::default()
        });
        assert!(resolve_input("rust async").starts_with("https://search.brave.com/"));
        set_app_settings(AppSettings::default());
        assert!(resolve_input("rust async").starts_with("https://duckduckgo.com/"));
    }

    #[test]
    fn navigable() {
        assert!(is_navigable("https://x.org"));
        assert!(!is_navigable("content://x"));
        assert!(!is_navigable("javascript:1"));
    }

    #[test]
    fn blocking_without_and_with_filters() {
        // Avant init : règles intégrées NyxGuard seulement.
        assert!(should_block("https://ad.doubleclick.net/x.js", "https://news.example/", "script"));
        // Jamais le document principal, jamais un scheme non-web.
        assert!(!should_block("https://ad.doubleclick.net/", "", "document"));
        assert!(!should_block("data:text/html,x", "", "script"));
        assert!(!should_block("https://duckduckgo.com/", "", "other"));

        let gz = gzip_stored(b"||ads.example^\n");
        assert!(init_filters(&[gz, b"corrompu".to_vec()]));
        assert!(filters_ready());
        assert!(should_block("https://ads.example/b.png", "https://site.example/", "image"));
        assert!(!should_block("https://site.example/b.png", "https://site.example/", "image"));
    }

    #[test]
    fn download_names() {
        assert_eq!(sanitize_filename("rapport.pdf"), "rapport.pdf");
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("a\\b\\photo.jpg"), "photo.jpg");
        assert_eq!(sanitize_filename("facture\u{202E}fdp.exe"), "facturefdp.exe");
        assert_eq!(sanitize_filename("x\0y.txt"), "xy.txt");
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename("   "), "download");
        assert_eq!(sanitize_filename("CON.txt"), "_CON.txt");
        assert!(sanitize_filename(&"a".repeat(400)).len() <= 255);
    }

    #[test]
    fn page_tokens() {
        assert!(!render_page("{{TOKENS}}").contains("{{"));
        assert_eq!(resource_type("https://x/a.js", ""), "script");
    }

    /// Archive gzip valide en mode « stored » (deflate sans compression),
    /// pour ne pas ajouter `flate2` en dev-dependency.
    fn gzip_stored(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
        let len = data.len() as u16;
        out.push(1); // BFINAL=1, BTYPE=00
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(data).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
}
