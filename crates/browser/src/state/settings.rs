use std::cell::RefCell;
use std::rc::Rc;

use crate::web::nyxwatch::NyxWatch;

pub type Settings = Rc<RefCell<AppSettings>>;

#[derive(Clone, Debug)]
pub struct AppSettings {
    pub search_engine:       SearchEngine,
    pub adblock_enabled:     bool,
    pub dark_websites:       bool,
    pub private_mode:        bool,
    pub block_third_party:   bool,
    pub on_last_tab:         LastTab,
    pub home_url:            String,
    pub language:            Language,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            search_engine:     SearchEngine::DuckDuckGo,
            adblock_enabled:   true,
            dark_websites:     false,
            private_mode:      false,
            block_third_party: false,
            on_last_tab:       LastTab::Home,
            home_url:          "nyx://newtab".into(),
            language:          Language::French,
        }
    }
}

pub fn new() -> Settings {
    Rc::new(RefCell::new(AppSettings::default()))
}

// ── Moteurs de recherche (privés par défaut — jamais Google/Bing) ──────────

#[derive(Clone, Debug, PartialEq)]
pub enum SearchEngine { DuckDuckGo, Brave, Ecosia }

impl SearchEngine {
    pub fn search_url(&self, query: &str) -> String {
        let q = urlencode(query);
        match self {
            Self::DuckDuckGo => format!("https://duckduckgo.com/?q={q}"),
            Self::Brave      => format!("https://search.brave.com/search?q={q}"),
            Self::Ecosia     => format!("https://www.ecosia.org/search?q={q}"),
        }
    }
    pub fn id(&self) -> &'static str {
        match self { Self::DuckDuckGo => "ddg", Self::Brave => "brave", Self::Ecosia => "ecosia" }
    }
    pub fn from_id(s: &str) -> Self {
        match s { "brave" => Self::Brave, "ecosia" => Self::Ecosia, _ => Self::DuckDuckGo }
    }
}

// ── Comportement à la fermeture du dernier onglet ──────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum LastTab { CloseWindow, Home }

impl LastTab {
    pub fn id(&self) -> &'static str {
        match self { Self::CloseWindow => "close", Self::Home => "home" }
    }
    pub fn from_id(s: &str) -> Self {
        match s { "close" => Self::CloseWindow, _ => Self::Home }
    }
}

// ── Langue ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Language { French, English, Spanish, German, Italian, Portuguese }

impl Language {
    pub fn id(&self) -> &'static str {
        match self {
            Self::French => "fr", Self::English => "en", Self::Spanish    => "es",
            Self::German => "de", Self::Italian => "it", Self::Portuguese => "pt",
        }
    }
    pub fn from_id(s: &str) -> Self {
        match s {
            "en" => Self::English, "es" => Self::Spanish, "de" => Self::German,
            "it" => Self::Italian, "pt" => Self::Portuguese, _ => Self::French,
        }
    }
}

// ── Application des réglages depuis nyx://apply?... ─────────────────────────

/// Parse `key=value&…` et applique aux réglages + au bloqueur. Retourne `true`
/// si la query n'était pas vide.
pub fn apply_from_url(url: &str, settings: &Settings, blocker: &NyxWatch) -> bool {
    let query = url.split_once('?').map(|x| x.1).unwrap_or("");
    if query.is_empty() {
        return false;
    }

    let params: std::collections::HashMap<&str, &str> = query
        .split('&')
        .filter_map(|pair| {
            let mut kv = pair.splitn(2, '=');
            Some((kv.next()?, kv.next().unwrap_or("")))
        })
        .collect();

    let flag = |k: &str| params.get(k).map(|v| *v == "true").unwrap_or(false);

    let mut s = settings.borrow_mut();
    if let Some(e) = params.get("engine") { s.search_engine = SearchEngine::from_id(e); }
    if let Some(h) = params.get("home")   { s.home_url = urldecode(h); }
    if let Some(l) = params.get("lang")   { s.language = Language::from_id(l); }
    if let Some(c) = params.get("lasttab"){ s.on_last_tab = LastTab::from_id(c); }

    // Checkboxes : présentes seulement si cochées → absent = false.
    s.adblock_enabled   = flag("adblock");
    s.dark_websites     = flag("dark");
    s.private_mode      = flag("private");
    s.block_third_party = flag("blockauth");

    blocker.set_enabled(s.adblock_enabled);
    blocker.set_block_accounts(s.block_third_party);
    true
}

fn urlencode(s: &str) -> String {
    s.replace('&', "%26").replace('#', "%23").replace(' ', "+")
}

fn urldecode(s: &str) -> String {
    s.replace('+', " ")
     .replace("%26", "&").replace("%23", "#")
     .replace("%3A", ":").replace("%2F", "/").replace("%3F", "?")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::nyxwatch::NyxWatch;

    fn setup() -> (Settings, NyxWatch) { (new(), NyxWatch::new()) }

    #[test]
    fn apply_roundtrip() {
        let (s, b) = setup();
        apply_from_url("nyx://apply?engine=brave&lang=es&dark=true&private=true&blockauth=true&lasttab=close&home=https%3A%2F%2Fx.com", &s, &b);
        let g = s.borrow();
        assert_eq!(g.search_engine, SearchEngine::Brave);
        assert_eq!(g.language, Language::Spanish);
        assert!(g.dark_websites && g.private_mode && g.block_third_party);
        assert_eq!(g.on_last_tab, LastTab::CloseWindow);
        assert_eq!(g.home_url, "https://x.com");
        assert!(!g.adblock_enabled); // absent → false
    }

    #[test]
    fn empty_query_noop() {
        let (s, b) = setup();
        assert!(!apply_from_url("nyx://apply", &s, &b));
    }

    /// « Centaines d'utilisateurs » : on martèle apply_from_url avec des
    /// combinaisons variées sur un état partagé. Doit rester cohérent et ne
    /// jamais paniquer (parsing robuste aux entrées mal formées).
    #[test]
    fn stress_hundreds_of_applies() {
        let (s, b) = setup();
        let engines = ["ddg", "brave", "ecosia", "garbage", ""];
        let langs   = ["fr", "en", "xx", "de", ""];
        for i in 0..500 {
            let eng  = engines[i % engines.len()];
            let lang = langs[i % langs.len()];
            let dark = if i % 2 == 0 { "true" } else { "false" };
            let url = format!(
                "nyx://apply?engine={eng}&lang={lang}&dark={dark}&adblock=true&blockauth={}&home=site{i}.com",
                i % 3 == 0
            );
            apply_from_url(&url, &s, &b);
            // Invariant : l'état reste lisible et l'adblock suit le flag.
            let g = s.borrow();
            assert!(g.adblock_enabled);
            assert_eq!(g.home_url, format!("site{i}.com"));
        }
    }

    /// Entrées hostiles / malformées : ne doivent jamais paniquer.
    #[test]
    fn stress_malformed_queries() {
        let (s, b) = setup();
        let weird = [
            "nyx://apply?",
            "nyx://apply?=&=&=",
            "nyx://apply?engine",
            "nyx://apply?engine=&&&lang",
            "nyx://apply?home=%%%%bad%encode",
            "nyx://apply?a=b&a=c&a=d",
            "nyx://apply?dark=TRUE&dark=true",
        ];
        for _ in 0..50 {
            for w in &weird {
                apply_from_url(w, &s, &b); // ne doit pas paniquer
            }
        }
    }
}
