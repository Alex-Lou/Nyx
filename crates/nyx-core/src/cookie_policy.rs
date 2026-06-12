//! CookiePolicy — politique pure pour la pose d'un cookie (`Set-Cookie`).
//!
//! Le browser appelle [`evaluate`] à chaque cookie que WebKit s'apprête à
//! persister. Il reçoit un [`Report`] détaillé (decision + raisons) qu'il
//! traduit côté `WebsiteDataManager` :
//!
//! - [`Decision::Allow`] : pose normale (persistant si `expires` valide).
//! - [`Decision::Strip`] : conservé pour la session courante uniquement
//!   (`max-age=0` côté disque, gardé en RAM).
//! - [`Decision::Block`] : rejeté avant écriture, ni RAM ni disque.
//!
//! Ce qu'il ne fait PAS :
//!   - aucun storage de cookies (zéro `HashMap<host, …>`).
//!   - pas de PSL complète : juste les TLD/eTLD+1 classiques pour la
//!     défense « Domain=.com ». Une PSL complète serait sur-dimensionnée
//!     ici — la décision finale revient à WebKit.
//!   - ne traite QUE la pose. L'envoi (cookie joint à une requête) reste
//!     du ressort du moteur réseau.
//!
//! Voir docs/security.md §X « Cookies ».

use crate::mode::Mode;
use crate::nyxguard;
use crate::sec_log::{self, Level};

// ─── Modèle ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

/// Attributs d'un cookie tels qu'extraits par WebKit du header `Set-Cookie`.
///
/// La VALEUR n'est PAS dans la struct : seule sa taille (`value_len`) est
/// nécessaire à la politique. Garantie zéro-fuite par construction.
#[derive(Debug, Clone)]
pub struct CookieAttrs<'a> {
    pub name: &'a str,
    pub value_len: usize,
    pub domain: &'a str,
    pub path: &'a str,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<SameSite>,
    /// `Max-Age` ou différence `Expires - now`, en secondes. `None` = cookie
    /// de session (browser-managed).
    pub expires_seconds: Option<i64>,
}

/// Contexte de pose : qui set, depuis où, dans quel mode.
#[derive(Debug, Clone)]
pub struct Context<'a> {
    /// Origine du document qui pose le cookie (`window.location` ou origin
    /// du response `Set-Cookie`).
    pub page_url: &'a str,
    /// Origine de la requête (utile pour les cookies tiers : URL du
    /// sub-resource qui répond).
    pub request_url: &'a str,
    pub mode: Mode,
    /// Le browser fournit ce flag (typiquement : eTLD+1 du `request_url`
    /// ≠ eTLD+1 du `page_url`).
    pub is_third_party: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Conservé pour la session courante (pas de persistance disque).
    Strip,
    Block,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub decision: Decision,
    pub reasons: Vec<Reason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Page HTTPS sans flag `Secure` → downgrade-friendly, on force session.
    MissingSecureOnHttps,
    /// `SameSite` absent (browsers défaut à Lax récemment) — signal seul.
    MissingSameSite,
    /// `SameSite=None` sans `Secure` → spec interdit explicitement.
    ImplicitSameSiteNone,
    /// 3rd-party host fait partie du blocklist tracker.
    ThirdPartyTracker,
    /// Value > 4096 octets — RFC 6265 plafond, et signe d'abus.
    OversizedValue,
    /// `Max-Age` > 400 jours — règle Chrome.
    OversizedExpiry,
    /// Page HTTP non-localhost — signal seul, browser le laisse passer.
    InsecureScheme,
    /// Page HTTP **réclamant** `Secure` — incohérent, refus sec.
    InsecureSecureClaim,
    /// `Domain=.com` ou autre TLD/eTLD+1 trop large.
    PublicSuffixDomain,
    /// Mode Shadow actif → tout cookie forcé en session.
    ShadowMode,
    /// Mode Banking actif — signal seul ; combiné avec 3rd-party = refus.
    BankingMode,
    /// Mode Banking + 3rd-party → refus sec.
    BankingThirdParty,
    /// Cookie sans nom — non-spec, refusé.
    EmptyName,
}

const MAX_VALUE_LEN:     usize = 4096;
const MAX_EXPIRY_SECONDS: i64  = 400 * 86_400;

// ─── API publique ──────────────────────────────────────────────────────────

/// Évalue une demande de pose. Pure : pas d'I/O, déterministe.
///
/// Sécurité par construction : on **accumule** les raisons, puis on
/// **synthétise** vers la décision la plus restrictive. Une seule raison
/// `Block` suffit pour bloquer, même si tout le reste est `Allow`.
pub fn evaluate(attrs: &CookieAttrs, ctx: &Context) -> Report {
    let mut reasons = Vec::new();

    if attrs.name.is_empty() {
        reasons.push(Reason::EmptyName);
    }

    let scheme = scheme_of(ctx.page_url);
    let is_https = scheme == "https";
    let is_http  = scheme == "http";
    let local    = is_localhost(ctx.page_url);

    // ─ Scheme / Secure cohérence ─
    if is_http && attrs.secure {
        reasons.push(Reason::InsecureSecureClaim);
    }
    if is_http && !local {
        reasons.push(Reason::InsecureScheme);
    }
    if is_https && !attrs.secure {
        reasons.push(Reason::MissingSecureOnHttps);
    }

    // ─ SameSite ─
    match attrs.same_site {
        None => reasons.push(Reason::MissingSameSite),
        Some(SameSite::None) if !attrs.secure => {
            reasons.push(Reason::ImplicitSameSiteNone);
        }
        _ => {}
    }

    // ─ Domain trop large ─
    if is_public_suffix(attrs.domain) {
        reasons.push(Reason::PublicSuffixDomain);
    }

    // ─ Taille ─
    if attrs.value_len > MAX_VALUE_LEN {
        reasons.push(Reason::OversizedValue);
    }

    // ─ Expiry ─
    if matches!(attrs.expires_seconds, Some(e) if e > MAX_EXPIRY_SECONDS) {
        reasons.push(Reason::OversizedExpiry);
    }

    // ─ 3rd-party tracker ─
    if ctx.is_third_party {
        let host = host_of(ctx.request_url);
        if !host.is_empty() && nyxguard::is_tracker_host(host) {
            reasons.push(Reason::ThirdPartyTracker);
        }
    }

    // ─ Mode ─
    match ctx.mode {
        Mode::Shadow  => reasons.push(Reason::ShadowMode),
        Mode::Banking => {
            reasons.push(Reason::BankingMode);
            if ctx.is_third_party {
                reasons.push(Reason::BankingThirdParty);
            }
        }
        Mode::Normal | Mode::Dev => {}
    }

    let decision = synthesize(&reasons);
    let report = Report { decision, reasons };
    log(&report, ctx);
    report
}

// ─── Synthèse ──────────────────────────────────────────────────────────────

/// Décision la plus restrictive parmi les raisons collectées. Default = Allow.
fn synthesize(reasons: &[Reason]) -> Decision {
    let mut d = Decision::Allow;
    for r in reasons {
        let r_dec = decision_for(r);
        if rank(r_dec) > rank(d) {
            d = r_dec;
        }
    }
    d
}

/// Décision impliquée par UNE raison isolée. Centralisé pour audit.
fn decision_for(r: &Reason) -> Decision {
    match r {
        Reason::ImplicitSameSiteNone
        | Reason::ThirdPartyTracker
        | Reason::OversizedValue
        | Reason::InsecureSecureClaim
        | Reason::PublicSuffixDomain
        | Reason::BankingThirdParty
        | Reason::EmptyName => Decision::Block,

        Reason::MissingSecureOnHttps
        | Reason::OversizedExpiry
        | Reason::ShadowMode => Decision::Strip,

        Reason::MissingSameSite
        | Reason::InsecureScheme
        | Reason::BankingMode => Decision::Allow,
    }
}

fn rank(d: Decision) -> u8 {
    match d {
        Decision::Allow => 0,
        Decision::Strip => 1,
        Decision::Block => 2,
    }
}

// ─── Helpers URL (locaux : on ne dépend pas d'`url::Url`) ──────────────────

fn scheme_of(url: &str) -> &str {
    let url = url.trim();
    url.find("://").map(|i| &url[..i]).unwrap_or("")
}

fn host_of(url: &str) -> &str {
    let url = url.trim();
    let after = url.find("://").map(|i| &url[i + 3..]).unwrap_or("");
    let end = after.find(['/', ':', '?', '#']).unwrap_or(after.len());
    &after[..end]
}

fn is_localhost(url: &str) -> bool {
    let h = host_of(url);
    h == "localhost" || h == "127.0.0.1" || h == "[::1]" || h.starts_with("127.")
}

/// `true` si `domain` (avec ou sans point de tête) est un suffixe public
/// (`.com`, `.co.uk`, …). Sert à bloquer `Domain=.com` qui ferait fuiter
/// le cookie à tout l'Internet.
fn is_public_suffix(domain: &str) -> bool {
    if domain.is_empty() { return false; }
    let d = domain.strip_prefix('.').unwrap_or(domain).to_ascii_lowercase();
    PUBLIC_SUFFIXES.iter().any(|s| d == *s)
}

/// Liste minimale — pas une PSL complète. Couvre les TLD courants et
/// quelques eTLD à 2 segments qui sont les pièges classiques. Étendre si
/// un faux négatif apparaît en pratique.
const PUBLIC_SUFFIXES: &[&str] = &[
    "com", "org", "net", "io", "co", "edu", "gov", "mil",
    "biz", "info", "name", "pro", "us", "ca", "uk", "fr",
    "de", "es", "it", "jp", "cn", "ru", "br", "au", "nl",
    "be", "ch", "se", "no", "dk", "fi", "pl", "cz", "at",
    "co.uk", "co.jp", "co.kr", "com.au", "com.br", "com.cn",
    "org.uk", "ac.uk", "gov.uk", "co.nz",
];

fn log(report: &Report, ctx: &Context) {
    let level = match report.decision {
        Decision::Allow => return,
        Decision::Strip => Level::Warn,
        Decision::Block => Level::Block,
    };
    // Raison principale : la première qui justifie la décision finale.
    let main = report.reasons.iter()
        .find(|r| decision_for(r) == report.decision)
        .map(|r| format!("{r:?}"))
        .unwrap_or_else(|| "Unspecified".into());
    sec_log::emit(level, &format!(
        "cookie {:?} on {}: {}",
        report.decision,
        sec_log::redact_url(ctx.page_url),
        main,
    ));
}

#[cfg(test)]
mod tests;
