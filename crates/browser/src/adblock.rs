// Sprint 3 — moteur `adblock` de Brave + EasyList/EasyPrivacy bundlées.
//
// Deux niveaux de blocage :
// - le moteur (ici) filtre les navigations via decide-policy ;
// - les sous-ressources (img, script, xhr…) sont bloquées par le
//   content filter WebKit compilé depuis les mêmes listes (content_filter.rs).

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Read;

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;

const EASYLIST_GZ: &[u8] = include_bytes!("../../../assets/filterlists/easylist.txt.gz");
const EASYPRIVACY_GZ: &[u8] = include_bytes!("../../../assets/filterlists/easyprivacy.txt.gz");

// Tout vit sur le thread GTK : Rc<AdBlocker> + RefCell, pas de verrou.
pub struct AdBlocker {
    engine: Engine,
    /// Hosts pour lesquels l'utilisateur a désactivé le blocage (Sprint 3.4).
    whitelist: RefCell<HashSet<String>>,
}

impl AdBlocker {
    pub fn new() -> Self {
        Self {
            engine: Engine::from_filter_set(bundled_filter_set(false), true),
            whitelist: RefCell::new(HashSet::new()),
        }
    }

    /// true si la navigation vers `url` (depuis `source_url`) doit être bloquée.
    pub fn should_block(&self, url: &str, source_url: &str, main_frame: bool) -> bool {
        let (target_host, _) = crate::urls::host_and_path(url);
        let (source_host, _) = crate::urls::host_and_path(source_url);
        if self.is_whitelisted(target_host) || self.is_whitelisted(source_host) {
            return false;
        }

        let source = if source_url.is_empty() { url } else { source_url };
        let kind = if main_frame { "document" } else { "subdocument" };
        Request::new(url, source, kind)
            .map(|r| self.engine.check_network_request(&r).matched)
            .unwrap_or(false)
    }

    pub fn is_whitelisted(&self, host: &str) -> bool {
        self.whitelist
            .borrow()
            .iter()
            .any(|domain| host_matches(host, domain))
    }

    pub fn set_whitelisted(&self, host: &str, allowed: bool) {
        let mut wl = self.whitelist.borrow_mut();
        if allowed {
            wl.insert(host.to_string());
        } else {
            wl.remove(host);
        }
    }

    pub fn load_whitelist(&self, domains: impl IntoIterator<Item = String>) {
        self.whitelist.borrow_mut().extend(domains);
    }

    pub fn whitelist_snapshot(&self) -> Vec<String> {
        let mut out: Vec<String> = self.whitelist.borrow().iter().cloned().collect();
        out.sort();
        out
    }
}

/// JSON content-blocker WebKit généré depuis les listes bundlées.
/// Coûteux (re-parse les listes) : appelé une seule fois, à la première
/// compilation du filtre (ensuite il est en cache disque).
/// Le mode debug est requis : la conversion a besoin du texte brut des règles.
pub fn content_blocking_json() -> String {
    match bundled_filter_set(true).into_content_blocking() {
        Ok((rules, _unsupported)) => serde_json::to_string(&rules).unwrap_or_else(|_| "[]".into()),
        Err(_) => "[]".into(),
    }
}

fn bundled_filter_set(debug: bool) -> FilterSet {
    let mut set = FilterSet::new(debug);
    for gz in [EASYLIST_GZ, EASYPRIVACY_GZ] {
        set.add_filters(gunzip(gz).lines(), ParseOptions::default());
    }
    set
}

fn gunzip(data: &[u8]) -> String {
    let mut out = String::new();
    let _ = flate2::read::GzDecoder::new(data).read_to_string(&mut out);
    out
}

/// true si `host` est `domain` ou un sous-domaine de `domain`.
fn host_matches(host: &str, domain: &str) -> bool {
    host.strip_suffix(domain)
        .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
}

impl Default for AdBlocker {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Un seul test : la construction du moteur parse ~3,5 MB de règles,
    // on ne la paie qu'une fois.
    #[test]
    fn moteur_easylist_et_whitelist() {
        let b = AdBlocker::new();

        // Règles EasyList réelles (iframe pub typique)
        const AD_IFRAME: &str = "https://googleads.g.doubleclick.net/pagead/ads?client=x";
        assert!(b.should_block(AD_IFRAME, "https://example.com", false));
        assert!(!b.should_block("https://duckduckgo.com", "", true));
        assert!(!b.should_block("https://rust-lang.org/learn", "https://rust-lang.org", true));

        // Whitelist : site source autorisé → plus de blocage
        b.set_whitelisted("example.com", true);
        assert!(b.is_whitelisted("example.com"));
        assert!(b.is_whitelisted("www.example.com"));
        assert!(!b.is_whitelisted("notexample.com"));
        assert!(!b.should_block(AD_IFRAME, "https://example.com", false));
        b.set_whitelisted("example.com", false);
        assert!(!b.is_whitelisted("example.com"));

        assert_eq!(b.whitelist_snapshot(), Vec::<String>::new());
    }

    #[test]
    fn correspondance_de_host() {
        assert!(host_matches("a.b.com", "b.com"));
        assert!(host_matches("b.com", "b.com"));
        assert!(!host_matches("notb.com", "b.com"));
    }
}
