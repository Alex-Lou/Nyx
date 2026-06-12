use crate::state::settings::AppSettings;

const TEMPLATE: &str = include_str!("../../../../assets/settings.html");

/// Rend la page paramètres avec les valeurs actuelles injectées dans le gabarit.
pub fn html(s: &AppSettings) -> String {
    let chk  = |id: &str, cur: &str| if id == cur { " checked" } else { "" };
    let flag = |on: bool| if on { " checked" } else { "" };

    TEMPLATE
        .replace("{{TOKENS}}",        crate::pages::tokens::ROOT)
        .replace("{{ENGINE_DDG}}",    chk("ddg",    s.search_engine.id()))
        .replace("{{ENGINE_BRAVE}}",  chk("brave",  s.search_engine.id()))
        .replace("{{ENGINE_ECOSIA}}", chk("ecosia", s.search_engine.id()))
        .replace("{{HOME_URL}}",      &s.home_url)
        .replace("{{ADBLOCK}}",       flag(s.adblock_enabled))
        .replace("{{BLOCKAUTH}}",     flag(s.block_third_party))
        .replace("{{PRIVATE}}",       flag(s.private_mode))
        .replace("{{DARK}}",          flag(s.dark_websites))
        .replace("{{LASTTAB_HOME}}",  chk("home",  s.on_last_tab.id()))
        .replace("{{LASTTAB_CLOSE}}", chk("close", s.on_last_tab.id()))
        .replace("{{LANG_FR}}",       chk("fr", s.language.id()))
        .replace("{{LANG_EN}}",       chk("en", s.language.id()))
        .replace("{{LANG_ES}}",       chk("es", s.language.id()))
        .replace("{{LANG_DE}}",       chk("de", s.language.id()))
        .replace("{{LANG_IT}}",       chk("it", s.language.id()))
        .replace("{{LANG_PT}}",       chk("pt", s.language.id()))
}
