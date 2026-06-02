use crate::state::settings::AppSettings;

const TEMPLATE: &str = include_str!("../../../../assets/settings.html");

/// Rend la page paramètres avec les valeurs actuelles injectées dans le gabarit.
pub fn html(s: &AppSettings) -> String {
    let chk = |id: &str, cur: &str| if id == cur { " checked" } else { "" };

    TEMPLATE
        .replace("{{ENGINE_DDG}}",    chk("ddg",    s.search_engine.id()))
        .replace("{{ENGINE_BRAVE}}",  chk("brave",  s.search_engine.id()))
        .replace("{{ENGINE_ECOSIA}}", chk("ecosia", s.search_engine.id()))
        .replace("{{HOME_URL}}",      &s.home_url)
        .replace("{{ADBLOCK}}",       if s.adblock_enabled { " checked" } else { "" })
        .replace("{{DARK}}",          if s.dark_websites   { " checked" } else { "" })
        .replace("{{LANG_FR}}",       chk("fr", s.language.id()))
        .replace("{{LANG_EN}}",       chk("en", s.language.id()))
        .replace("{{LANG_ES}}",       chk("es", s.language.id()))
        .replace("{{LANG_DE}}",       chk("de", s.language.id()))
        .replace("{{LANG_IT}}",       chk("it", s.language.id()))
        .replace("{{LANG_PT}}",       chk("pt", s.language.id()))
}
