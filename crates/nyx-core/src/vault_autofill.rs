//! VaultAutofillGuard — décide quand Nyx peut pré-remplir des credentials.
//!
//! **Politique pure**. Le browser interroge ce module AVANT d'afficher
//! quoi que ce soit (suggestion d'autofill, popup vault) — surtout AVANT
//! d'envoyer la moindre donnée au DOM.
//!
//! Surface critique : un autofill mal placé envoie un mot de passe au pire
//! endroit possible. Default = **Deny**. Une seule règle qui rate suffit pour
//! que la décision soit `Deny` ou `Ask` — jamais `Allow` par défaut.
//!
//! Voir docs/security.md §2 « Vault & autofill ».

use crate::domain_risk::{analyze_url, Risk};
use crate::sec_log::{self, Level};

// ─── Modèle ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Pré-remplissage silencieux autorisé (badge discret au-dessus du champ).
    Allow,
    /// Proposer mais demander confirmation explicite (clic utilisateur).
    Ask,
    /// **Aucune** action UI : pas de suggestion, pas de fuite d'info.
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Identifiant principal (login/email).
    UsernameOrEmail,
    /// Mot de passe — exigences les plus strictes.
    Password,
    /// Numéro de carte de paiement — encore plus strict.
    CreditCard,
    /// Autres champs sensibles (TOTP code, secret).
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    /// Toujours `Ask`, jamais `Allow` silencieux.
    Shadow,
    /// Match exact requis + jamais d'autofill silencieux.
    Banking,
}

#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// URL de la page qui héberge le formulaire (`window.location`).
    pub page_url: &'a str,
    /// URL de destination du `<form action="…">` (= où partent les credentials).
    /// Si vide, on suppose même origine que la page.
    pub form_action_url: &'a str,
    /// Origine canonique enregistrée pour la credential vault
    /// (scheme + host + port par défaut retiré). Vide = pas d'entrée vault.
    pub vault_origin: &'a str,
    pub field: FieldKind,
    pub mode: Mode,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub decision: Decision,
    pub reasons: Vec<Reason>,
    /// Origine canonique de la page (pour log + UI).
    pub page_origin: String,
    /// Origine canonique du form action (pour log + UI).
    pub form_origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Page http:// — credential interceptable. Refus sec.
    HttpPage,
    /// `<form action>` pointe sur http:// → fuite en clair.
    HttpFormAction,
    /// Origine de la page = Dangerous (IDN homographe, mixed-script).
    DangerousPage,
    /// Origine de la page = Suspicious (IDN propre).
    SuspiciousPage,
    /// Aucune entrée vault enregistrée pour cette origine.
    NoVaultEntry,
    /// page_origin ≠ vault_origin (origines différentes).
    OriginMismatch,
    /// form_action_origin ≠ page_origin : le form envoie ailleurs.
    /// Classique : page sur paypal.com, action vers evil.com.
    FormActionOffOrigin,
    /// Sous-domaine non strict : vault = `example.com`, page = `sub.example.com`.
    /// On peut décider Allow ou Ask selon mode ; raison enregistrée pour la trace.
    SubdomainMatchOnly,
    /// Champ carte de paiement → toujours Ask au minimum.
    CreditCardField,
    /// Mode Banking actif → policy stricte.
    BankingMode,
    /// Mode Shadow actif → jamais d'Allow silencieux.
    ShadowMode,
}

// ─── API publique ──────────────────────────────────────────────────────────

/// Analyse une demande d'autofill et rend un verdict détaillé.
///
/// Pure : pas d'I/O, déterministe. Sécurité par construction : on accumule
/// les raisons puis on agrège vers la décision **la plus restrictive**.
pub fn decide(req: &Request) -> Report {
    let mut reasons = Vec::new();

    let page_origin = canonical_origin(req.page_url);
    let form_origin = if req.form_action_url.is_empty() {
        page_origin.clone()
    } else {
        canonical_origin(req.form_action_url)
    };

    collect_origin_risks(req, &page_origin, &form_origin, &mut reasons);
    collect_mode_constraints(req.mode, req.field, &mut reasons);
    collect_vault_match(req.vault_origin, &page_origin, &mut reasons);

    let decision = synthesize_decision(&reasons);
    let report = Report { decision, reasons, page_origin, form_origin };
    log(&report, req);
    report
}

// ─── Collecte de raisons ───────────────────────────────────────────────────

/// Origines : HTTP, IDN risk, form action off-origin.
fn collect_origin_risks(
    req: &Request, page_origin: &str, form_origin: &str, out: &mut Vec<Reason>,
) {
    if req.page_url.starts_with("http://") && !is_localhost(page_origin) {
        out.push(Reason::HttpPage);
    }
    if !req.form_action_url.is_empty()
        && req.form_action_url.starts_with("http://")
        && !is_localhost(form_origin)
    {
        out.push(Reason::HttpFormAction);
    }
    if !page_origin.is_empty() && !form_origin.is_empty() && page_origin != form_origin {
        out.push(Reason::FormActionOffOrigin);
    }
    match analyze_url(req.page_url).map(|a| a.risk) {
        Some(Risk::Dangerous)  => out.push(Reason::DangerousPage),
        Some(Risk::Suspicious) => out.push(Reason::SuspiciousPage),
        _ => {}
    }
}

/// Politique par mode + type de champ.
fn collect_mode_constraints(mode: Mode, field: FieldKind, out: &mut Vec<Reason>) {
    match mode {
        Mode::Shadow  => out.push(Reason::ShadowMode),
        Mode::Banking => out.push(Reason::BankingMode),
        Mode::Normal  => {}
    }
    if matches!(field, FieldKind::CreditCard) {
        out.push(Reason::CreditCardField);
    }
}

/// Match entre l'origine de la page et celle de l'entrée vault.
fn collect_vault_match(vault_origin: &str, page_origin: &str, out: &mut Vec<Reason>) {
    if vault_origin.is_empty() {
        out.push(Reason::NoVaultEntry);
        return;
    }
    if page_origin == vault_origin {
        return; // match exact = silencieux, pas de raison ajoutée
    }
    if is_subdomain_of(page_origin, vault_origin)
        || is_subdomain_of(vault_origin, page_origin)
    {
        out.push(Reason::SubdomainMatchOnly);
    } else {
        out.push(Reason::OriginMismatch);
    }
}

// ─── Synthèse ──────────────────────────────────────────────────────────────

/// Agrège : on prend la décision **la plus restrictive** parmi les raisons.
/// Default = Allow ; chaque raison peut downgrade vers Ask ou Deny.
fn synthesize_decision(reasons: &[Reason]) -> Decision {
    let mut d = Decision::Allow;
    for r in reasons {
        let r_dec = decision_for(r);
        if more_restrictive(r_dec, d) { d = r_dec; }
    }
    d
}

/// Décision impliquée par UNE raison isolée.
fn decision_for(r: &Reason) -> Decision {
    match r {
        // Refus secs : on n'envoie JAMAIS de credentials en clair, jamais
        // sur un domaine pourri, et jamais à une origine tierce.
        Reason::HttpPage
        | Reason::HttpFormAction
        | Reason::DangerousPage
        | Reason::FormActionOffOrigin
        | Reason::OriginMismatch
        | Reason::NoVaultEntry => Decision::Deny,

        // Confirmation user requise.
        Reason::SuspiciousPage
        | Reason::SubdomainMatchOnly
        | Reason::CreditCardField
        | Reason::BankingMode
        | Reason::ShadowMode => Decision::Ask,
    }
}

fn more_restrictive(a: Decision, b: Decision) -> bool {
    rank(a) > rank(b)
}

fn rank(d: Decision) -> u8 {
    match d {
        Decision::Allow => 0,
        Decision::Ask   => 1,
        Decision::Deny  => 2,
    }
}

// ─── Helpers origine (mêmes règles que site_data_policy, sans dupliquer
// la fonction au prix d'un coupling : on reste local et explicite). ────────

/// `https://user:pw@host:443/p?q` → `https://host`.
fn canonical_origin(url: &str) -> String {
    let s = url.trim();
    let (scheme, default_port) = if s.starts_with("https://") { ("https", "443") }
        else if s.starts_with("http://") { ("http", "80") }
        else { return String::new() };
    let after = &s[scheme.len() + 3..];
    let end = after.find('/').unwrap_or(after.len());
    let authority = after[..end].rsplit('@').next().unwrap_or(&after[..end]);
    let host = match authority.rsplit_once(':') {
        Some((h, p)) if p == default_port => h,
        _ => authority,
    };
    if host.is_empty() { String::new() } else { format!("{scheme}://{host}") }
}

fn is_localhost(origin: &str) -> bool {
    origin.contains("//localhost") || origin.contains("//127.")
        || origin.contains("//[::1]")
}

/// `https://sub.example.com` est un sous-domaine de `https://example.com` (même scheme).
fn is_subdomain_of(child: &str, parent: &str) -> bool {
    let Some(c_host) = child.split("://").nth(1) else { return false };
    let Some(p_host) = parent.split("://").nth(1) else { return false };
    let c_scheme = child.split("://").next().unwrap_or("");
    let p_scheme = parent.split("://").next().unwrap_or("");
    c_scheme == p_scheme && c_host != p_host && c_host.ends_with(&format!(".{p_host}"))
}

fn log(report: &Report, req: &Request) {
    let level = match report.decision {
        Decision::Allow => Level::Allow,
        Decision::Ask   => Level::Warn,
        Decision::Deny  => Level::Deny,
    };
    sec_log::emit(level, &format!(
        "vault autofill {:?} {:?}: page={} form={} reasons={}",
        report.decision, req.field,
        report.page_origin, report.form_origin, report.reasons.len(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(page: &'static str, form: &'static str, vault: &'static str) -> Request<'static> {
        Request {
            page_url: page, form_action_url: form, vault_origin: vault,
            field: FieldKind::Password, mode: Mode::Normal,
        }
    }

    // ─── Cas nominal : exact match HTTPS, même origine ──────────────

    #[test]
    fn exact_match_allows() {
        let r = decide(&req("https://example.com/login", "", "https://example.com"));
        assert_eq!(r.decision, Decision::Allow);
        assert!(r.reasons.is_empty());
    }

    #[test]
    fn exact_match_with_form_action_same_origin_allows() {
        let r = decide(&req("https://example.com/login",
                            "https://example.com/auth", "https://example.com"));
        assert_eq!(r.decision, Decision::Allow);
    }

    // ─── HTTP / downgrade ───────────────────────────────────────────

    #[test]
    fn http_page_denies() {
        let r = decide(&req("http://example.com/login", "", "https://example.com"));
        assert_eq!(r.decision, Decision::Deny);
        assert!(r.reasons.contains(&Reason::HttpPage));
    }

    #[test]
    fn http_form_action_denies() {
        let r = decide(&req("https://example.com/login",
                            "http://example.com/auth", "https://example.com"));
        assert_eq!(r.decision, Decision::Deny);
        assert!(r.reasons.contains(&Reason::HttpFormAction));
    }

    #[test]
    fn http_localhost_dev_allowed() {
        let r = decide(&req("http://localhost:3000/login", "", "http://localhost:3000"));
        assert_eq!(r.decision, Decision::Allow);
    }

    // ─── IDN homograph ──────────────────────────────────────────────

    #[test]
    fn dangerous_idn_denies_even_with_vault_entry() {
        // L'attaquant a réussi à faire qu'on a une entrée vault pour le faux.
        // Le module DENY quand même parce que l'origine est Dangerous.
        let r = decide(&req("https://pаypal.com/login", "", "https://pаypal.com"));
        assert_eq!(r.decision, Decision::Deny);
        assert!(r.reasons.contains(&Reason::DangerousPage));
    }

    #[test]
    fn suspicious_idn_asks() {
        let r = decide(&req("https://bücher.de/login", "", "https://bücher.de"));
        assert_eq!(r.decision, Decision::Ask);
        assert!(r.reasons.contains(&Reason::SuspiciousPage));
    }

    // ─── Form action vers ailleurs (vol de credentials classique) ──

    #[test]
    fn form_action_off_origin_denies() {
        let r = decide(&req("https://paypal.com/login",
                            "https://evil.com/grab", "https://paypal.com"));
        assert_eq!(r.decision, Decision::Deny);
        assert!(r.reasons.contains(&Reason::FormActionOffOrigin));
    }

    // ─── Match origine vault ────────────────────────────────────────

    #[test]
    fn no_vault_entry_denies() {
        let r = decide(&req("https://example.com/login", "", ""));
        assert_eq!(r.decision, Decision::Deny);
        assert!(r.reasons.contains(&Reason::NoVaultEntry));
    }

    #[test]
    fn origin_mismatch_denies() {
        let r = decide(&req("https://example.com/login", "", "https://other.com"));
        assert_eq!(r.decision, Decision::Deny);
        assert!(r.reasons.contains(&Reason::OriginMismatch));
    }

    #[test]
    fn subdomain_match_asks() {
        // Vault = example.com, page = accounts.example.com → Ask (pas Allow).
        let r = decide(&req("https://accounts.example.com/login", "",
                            "https://example.com"));
        assert_eq!(r.decision, Decision::Ask);
        assert!(r.reasons.contains(&Reason::SubdomainMatchOnly));
    }

    #[test]
    fn subdomain_reverse_match_asks() {
        // Vault = sub.example.com, page = example.com → idem.
        let r = decide(&req("https://example.com/login", "",
                            "https://sub.example.com"));
        assert_eq!(r.decision, Decision::Ask);
    }

    // ─── Types de champ ─────────────────────────────────────────────

    #[test]
    fn credit_card_always_asks_minimum() {
        let mut r = req("https://example.com/", "", "https://example.com");
        r.field = FieldKind::CreditCard;
        let report = decide(&r);
        assert_eq!(report.decision, Decision::Ask);
        assert!(report.reasons.contains(&Reason::CreditCardField));
    }

    // ─── Modes ──────────────────────────────────────────────────────

    #[test]
    fn shadow_mode_always_asks() {
        let mut r = req("https://example.com/", "", "https://example.com");
        r.mode = Mode::Shadow;
        assert_eq!(decide(&r).decision, Decision::Ask);
    }

    #[test]
    fn banking_mode_always_asks_minimum() {
        let mut r = req("https://bank.com/", "", "https://bank.com");
        r.mode = Mode::Banking;
        assert_eq!(decide(&r).decision, Decision::Ask);
    }

    #[test]
    fn banking_mode_denies_if_origin_mismatch() {
        // Banking + origin mismatch = Deny (le mismatch domine sur Ask).
        let mut r = req("https://bank.com/", "", "https://other.com");
        r.mode = Mode::Banking;
        assert_eq!(decide(&r).decision, Decision::Deny);
    }

    // ─── Combinaison : la décision la PLUS restrictive l'emporte ────

    #[test]
    fn http_plus_suspicious_still_deny() {
        let r = decide(&req("http://bücher.de/login", "", "https://bücher.de"));
        assert_eq!(r.decision, Decision::Deny); // HttpPage > Suspicious
    }

    #[test]
    fn shadow_plus_dangerous_still_deny() {
        let mut r = req("https://pаypal.com/", "", "https://pаypal.com");
        r.mode = Mode::Shadow;
        assert_eq!(decide(&r).decision, Decision::Deny);
    }

    // ─── Origines extraites ─────────────────────────────────────────

    #[test]
    fn origin_extraction_strips_default_port_and_userinfo() {
        let r = decide(&req("https://user:pw@example.com:443/login", "",
                            "https://example.com"));
        assert_eq!(r.page_origin, "https://example.com");
        assert_eq!(r.decision, Decision::Allow);
    }

    // ─── Panic test ─────────────────────────────────────────────────

    #[test]
    fn malformed_doesnt_panic() {
        let urls = ["", "://", "http://", "https://", "https://@/", "javascript:x",
                    "file:///etc/passwd", "https:///path", "ws://x", "\0\n"];
        for p in &urls {
            for f in &urls {
                for v in &urls {
                    let _ = decide(&req(p, f, v));
                }
            }
        }
    }

    // ─── Stress ─────────────────────────────────────────────────────

    #[test]
    fn stress_thousands_of_decisions() {
        let pages = ["https://example.com/", "https://pаypal.com/",
                     "http://bank.com/", "https://accounts.example.com/",
                     "https://bücher.de/login"];
        let forms = ["", "https://example.com/auth", "https://evil.com/grab"];
        let vaults = ["https://example.com", "https://bank.com", ""];
        for i in 0..5_000 {
            let r = Request {
                page_url:        pages[i % pages.len()],
                form_action_url: forms[i % forms.len()],
                vault_origin:    vaults[i % vaults.len()],
                field: if i % 2 == 0 { FieldKind::Password } else { FieldKind::CreditCard },
                mode:  match i % 3 { 0 => Mode::Normal, 1 => Mode::Shadow, _ => Mode::Banking },
            };
            let _ = decide(&r);
        }
    }
}
