//! Script de pré-remplissage partagé (Windows WebView2, Android WebView) :
//! décision [`super::decide`] + garde-fous propres au remplissage silencieux.
//! Pur, testé sur l'hôte.
//!
//! Garde-fous en plus de la politique :
//! • https uniquement, hôte EXACT (pas de sous-domaine), un seul compte
//!   enregistré (plusieurs → l'utilisateur choisit lui-même) ;
//! • le script revérifie `location.origin` (course avec une navigation),
//!   refuse les iframes, les formulaires dont l'`action` part vers une autre
//!   origine et les champs `new-password` ; il ne remplit que des champs
//!   présents, vides et visibles, une seule fois (≤ 10 s d'observation pour
//!   les SPA).

use super::{decide, Decision, FieldKind, Mode, Request};
use crate::url::{host_of, origin_of, scheme_of};

const FILL_JS: &str = r#"(function (o, u, p) {
    if (window.top !== window || location.origin !== o) return;
    var done = false;
    function visible(el) {
        return !!(el.offsetWidth || el.offsetHeight || el.getClientRects().length);
    }
    function sameOrigin(form) {
        var a = form && form.getAttribute('action');
        if (!a) return true;
        try { return new URL(a, location.href).origin === location.origin; } catch (_) { return false; }
    }
    function set(el, v) {
        var d = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value');
        d.set.call(el, v);
        el.dispatchEvent(new Event('input', { bubbles: true }));
        el.dispatchEvent(new Event('change', { bubbles: true }));
    }
    function attempt() {
        if (done) return true;
        var pw = Array.prototype.filter.call(document.querySelectorAll('input[type="password"]'), function (i) {
            return visible(i) && !i.value && !i.disabled && !i.readOnly &&
                (i.getAttribute('autocomplete') || '').toLowerCase() !== 'new-password';
        })[0];
        if (!pw || !sameOrigin(pw.form)) return false;
        var scope = pw.form || document;
        var user = Array.prototype.filter.call(scope.querySelectorAll('input'), function (i) {
            return (i.type === 'text' || i.type === 'email' || i.type === 'tel') && visible(i) && !i.disabled &&
                (i.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING);
        }).pop();
        done = true;
        if (user && u && !user.value) set(user, u);
        set(pw, p);
        return true;
    }
    if (attempt()) return;
    var obs = new MutationObserver(function () { if (attempt()) obs.disconnect(); });
    obs.observe(document.documentElement, { childList: true, subtree: true });
    setTimeout(function () { obs.disconnect(); }, 10000);
})"#;

/// Identifiant enregistré pour l'hôte exact de la page.
#[derive(Debug, Clone, Copy)]
pub struct Credential<'a> {
    pub domain: &'a str,
    pub username: &'a str,
    pub password: &'a str,
}

/// Littéral JS sûr (JSON + séparateurs de ligne Unicode et `<` échappés :
/// jamais de `</script>` ni de fin de ligne JS dans une chaîne).
pub fn js_literal(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap_or_else(|_| "null".into())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .replace('<', "\\u003c")
}

/// Script de pré-remplissage pour `page_url`, ou `None` si la politique ne
/// l'autorise pas silencieusement. `creds` : identifiants du coffre pour
/// l'hôte exact de la page. `private` : navigation privée (mode Shadow →
/// jamais d'autofill silencieux). Les pages internes de chaque coquille
/// sont exclues par l'appelant.
pub fn fill_script(page_url: &str, creds: &[Credential], private: bool) -> Option<String> {
    if scheme_of(page_url).as_deref() != Some("https") {
        return None;
    }
    let host = host_of(page_url)?;
    let [cred] = creds else { return None };
    if !cred.domain.eq_ignore_ascii_case(&host) || cred.password.is_empty() {
        return None;
    }
    let vault_origin = format!("https://{}", cred.domain.to_ascii_lowercase());
    let report = decide(&Request {
        page_url,
        form_action_url: "",
        vault_origin: &vault_origin,
        field: FieldKind::Password,
        mode: if private { Mode::Shadow } else { Mode::Normal },
    });
    if report.decision != Decision::Allow {
        return None;
    }
    let origin = origin_of(page_url)?;
    Some(format!(
        "{FILL_JS}({},{},{})",
        js_literal(&origin),
        js_literal(cred.username),
        js_literal(cred.password)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred<'a>(domain: &'a str, username: &'a str, password: &'a str) -> Credential<'a> {
        Credential { domain, username, password }
    }

    #[test]
    fn fills_exact_https_host() {
        let js = fill_script("https://example.com/login", &[cred("example.com", "alex", "p\"w</script>")], false)
            .expect("autorisé");
        assert!(js.ends_with(r#"("https://example.com","alex","p\"w\u003c/script>")"#), "{js}");
        assert!(js.contains("window.top !== window"));
    }

    #[test]
    fn refuses_unsafe_contexts() {
        let c = [cred("example.com", "alex", "pw")];
        assert_eq!(fill_script("http://example.com/login", &c, false), None, "http");
        assert_eq!(fill_script("https://evil.com/login", &c, false), None, "autre hôte");
        assert_eq!(fill_script("https://sub.example.com/", &c, false), None, "sous-domaine");
        assert_eq!(fill_script("https://example.com:8443/", &c, false), None, "autre port");
        assert_eq!(fill_script("https://example.com/", &c, true), None, "navigation privée");
        assert_eq!(fill_script("file:///C:/x.html", &c, false), None);
        assert_eq!(fill_script("https://example.com/", &[], false), None, "rien en coffre");
        assert_eq!(
            fill_script("https://example.com/", &[cred("example.com", "a", "1"), cred("example.com", "b", "2")], false),
            None,
            "plusieurs comptes : pas de choix silencieux"
        );
        assert_eq!(fill_script("https://example.com/", &[cred("example.com", "a", "")], false), None);
        // Homographe IDN : refusé par la politique même avec une entrée.
        assert_eq!(fill_script("https://pаypal.com/", &[cred("pаypal.com", "a", "b")], false), None);
    }

    #[test]
    fn literal_escapes_line_separators() {
        assert_eq!(js_literal("a\u{2028}b<"), r#""a\u2028b\u003c""#);
    }
}
