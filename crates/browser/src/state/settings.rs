use std::cell::RefCell;
use std::rc::Rc;

use crate::web::adblock::AdBlocker;

pub type Settings = Rc<RefCell<AppSettings>>;

#[derive(Clone, Debug)]
pub struct AppSettings {
    pub search_engine:   SearchEngine,
    pub adblock_enabled: bool,
    pub dark_websites:   bool,
    pub home_url:        String,
    pub language:        Language,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            search_engine:   SearchEngine::DuckDuckGo,
            adblock_enabled: true,
            dark_websites:   false,
            home_url:        "nyx://newtab".into(),
            language:        Language::French,
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

/// Parse `key=value&…` et applique. Retourne `true` si la query n'était pas vide.
pub fn apply_from_url(url: &str, settings: &Settings, blocker: &AdBlocker) -> bool {
    let query = url.splitn(2, '?').nth(1).unwrap_or("");
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

    let mut s = settings.borrow_mut();
    if let Some(e) = params.get("engine") { s.search_engine = SearchEngine::from_id(e); }
    if let Some(h) = params.get("home")   { s.home_url = urldecode(h); }
    if let Some(l) = params.get("lang")   { s.language = Language::from_id(l); }

    // Checkboxes : présentes seulement si cochées → absent = false.
    let adblock_on = params.get("adblock").map(|v| *v == "true").unwrap_or(false);
    s.adblock_enabled = adblock_on;
    blocker.set_enabled(adblock_on);

    s.dark_websites = params.get("dark").map(|v| *v == "true").unwrap_or(false);
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
