use std::cell::RefCell;
use std::rc::Rc;

pub type Settings = Rc<RefCell<AppSettings>>;

#[derive(Clone, Debug)]
pub struct AppSettings {
    pub search_engine:   SearchEngine,
    pub adblock_enabled: bool,
    pub home_url:        String,
    pub language:        Language,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            search_engine:   SearchEngine::DuckDuckGo,
            adblock_enabled: true,
            home_url:        "nyx://newtab".into(),
            language:        Language::French,
        }
    }
}

// ── Moteurs de recherche (privé par défaut — pas de Google/Bing) ────────

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
    pub fn base_url(&self) -> &'static str {
        match self {
            Self::DuckDuckGo => "https://duckduckgo.com",
            Self::Brave      => "https://search.brave.com",
            Self::Ecosia     => "https://www.ecosia.org",
        }
    }
    pub fn id(&self) -> &'static str {
        match self { Self::DuckDuckGo => "ddg", Self::Brave => "brave", Self::Ecosia => "ecosia" }
    }
    pub fn from_id(s: &str) -> Self {
        match s { "brave" => Self::Brave, "ecosia" => Self::Ecosia, _ => Self::DuckDuckGo }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::DuckDuckGo => "🦆 DuckDuckGo",
            Self::Brave      => "🦁 Brave Search",
            Self::Ecosia     => "🌳 Ecosia",
        }
    }
}

// ── Langue ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Language { French, English, Spanish, German, Italian, Portuguese }

impl Language {
    pub fn id(&self) -> &'static str {
        match self {
            Self::French     => "fr", Self::English   => "en", Self::Spanish  => "es",
            Self::German     => "de", Self::Italian   => "it", Self::Portuguese => "pt",
        }
    }
    pub fn from_id(s: &str) -> Self {
        match s {
            "en" => Self::English, "es" => Self::Spanish, "de" => Self::German,
            "it" => Self::Italian, "pt" => Self::Portuguese, _  => Self::French,
        }
    }
}

// ── Constructeur ─────────────────────────────────────────────────────────

pub fn new() -> Settings {
    Rc::new(RefCell::new(AppSettings::default()))
}

fn urlencode(s: &str) -> String {
    s.replace('&', "%26").replace('#', "%23").replace(' ', "+")
}

/// Parse `key=value&...` depuis une URL `nyx://apply?...` et applique les
/// changements à `settings`. Retourne `true` si au moins un param reconnu.
pub fn apply_from_url(url: &str, settings: &Settings, blocker: &crate::adblock::AdBlocker) -> bool {
    let query = url.splitn(2, '?').nth(1).unwrap_or("");
    if query.is_empty() { return false; }

    let params: std::collections::HashMap<&str, &str> = query
        .split('&')
        .filter_map(|pair| {
            let mut kv = pair.splitn(2, '=');
            Some((kv.next()?, kv.next().unwrap_or("")))
        })
        .collect();

    let mut s = settings.borrow_mut();
    if let Some(e) = params.get("engine")  { s.search_engine = SearchEngine::from_id(e); }
    if let Some(h) = params.get("home")    { s.home_url = urldecode(h); }
    if let Some(l) = params.get("lang")    { s.language = Language::from_id(l); }

    let adblock_on = params.get("adblock").map(|v| *v == "true").unwrap_or(false);
    s.adblock_enabled = adblock_on;
    blocker.set_enabled(adblock_on);
    true
}

fn urldecode(s: &str) -> String {
    s.replace('+', " ")
     .replace("%26", "&")
     .replace("%23", "#")
     .replace("%3A", ":")
     .replace("%2F", "/")
     .replace("%3F", "?")
}
