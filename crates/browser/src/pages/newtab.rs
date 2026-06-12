use crate::pages::tokens;

const HTML: &str = include_str!("../../../../assets/newtab.html");

pub fn html() -> String {
    HTML.replace("{{TOKENS}}", tokens::ROOT)
}
