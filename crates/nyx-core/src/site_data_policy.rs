//! Politique de "Forget this site" — décide **quoi effacer** et **quel scope**
//! (origin exact vs domaine entier). Pure logic, sans WebKit ni I/O.
//!
//! Sprint suivant : intégration avec le vault (oublier le site = retirer aussi
//! ses credentials) et les permissions stockées.

use crate::url::{host_of, origin_of};

/// Granularité de l'oubli demandée par l'utilisateur.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Scope {
    /// `https://sub.example.com` exact — ne touche pas aux autres origines.
    Origin,
    /// `example.com` et tous ses sous-domaines / schémas.
    Domain,
}

/// Ce qui doit être effacé. Le caller (couche WebKit) lit ces flags et
/// appelle les API correspondantes.
#[derive(Debug, Clone)]
pub struct Plan {
    pub scope:        Scope,
    /// Origine (https://example.com) ou domaine (example.com) selon `scope`.
    pub target:       String,
    pub clear_cookies:    bool,
    pub clear_storage:    bool,    // localStorage + IndexedDB
    pub clear_cache:      bool,
    pub clear_workers:    bool,    // service workers, application cache
    pub clear_permissions: bool,   // permissions accordées (cam/mic/géoloc)
}

/// Construit un plan complet à partir de l'URL courante et du scope demandé.
/// Retourne `None` si l'URL n'a pas d'host valide (nyx://, file://, etc.).
pub fn plan_for(url: &str, scope: Scope) -> Option<Plan> {
    let target = match scope {
        Scope::Origin => origin_of(url)?,
        Scope::Domain => etld_plus_one(&host_of(url)?)?,
    };
    Some(Plan {
        scope,
        target,
        clear_cookies:     true,
        clear_storage:     true,
        clear_cache:       true,
        clear_workers:     true,
        clear_permissions: true,
    })
}

/// Heuristique eTLD+1 pragmatique (sans Public Suffix List).
/// `sub.example.com` → `example.com`, `a.b.co.uk` → `b.co.uk`.
///
/// Reconnaît une liste courte de TLDs composés très courants. Le cas non-couvert
/// (ex: `a.b.compute.amazonaws.com`) reste sûr : on prend les 2 derniers labels,
/// ce qui n'est pas trop large pour un "Forget".
fn etld_plus_one(host: &str) -> Option<String> {
    if host.parse::<std::net::IpAddr>().is_ok() || host == "localhost" {
        return Some(host.to_string());
    }
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() < 2 { return Some(host.to_string()); }

    // TLDs composés courants : si on les détecte en queue, on prend 3 labels.
    let tail2 = format!("{}.{}", labels[labels.len() - 2], labels[labels.len() - 1]);
    if MULTI_PART_TLDS.contains(&tail2.as_str()) && labels.len() >= 3 {
        return Some(labels[labels.len() - 3..].join("."));
    }
    Some(labels[labels.len() - 2..].join("."))
}

const MULTI_PART_TLDS: &[&str] = &[
    "co.uk", "co.jp", "co.kr", "co.in", "co.nz", "co.za",
    "com.au", "com.br", "com.mx", "com.cn", "com.sg",
    "org.uk", "ac.uk", "gov.uk", "net.au",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_scope_extracts_origin() {
        let p = plan_for("https://sub.example.com:443/path?x=1", Scope::Origin).unwrap();
        assert_eq!(p.target, "https://sub.example.com");
        assert_eq!(p.scope, Scope::Origin);
        assert!(p.clear_cookies && p.clear_storage && p.clear_cache);
    }

    #[test]
    fn domain_scope_strips_subdomain() {
        let p = plan_for("https://sub.example.com/x", Scope::Domain).unwrap();
        assert_eq!(p.target, "example.com");
    }

    #[test]
    fn domain_scope_handles_compound_tld() {
        let p = plan_for("https://www.bbc.co.uk/news", Scope::Domain).unwrap();
        assert_eq!(p.target, "bbc.co.uk");
        let p = plan_for("https://shop.example.com.au/", Scope::Domain).unwrap();
        assert_eq!(p.target, "example.com.au");
    }

    #[test]
    fn ip_and_localhost_stay_intact() {
        assert_eq!(plan_for("http://127.0.0.1:8080/", Scope::Domain).unwrap().target, "127.0.0.1");
        assert_eq!(plan_for("http://localhost:3000/", Scope::Domain).unwrap().target, "localhost");
    }

    #[test]
    fn rejects_internal_schemes() {
        assert!(plan_for("nyx://newtab", Scope::Origin).is_none());
        assert!(plan_for("file:///etc/passwd", Scope::Origin).is_none());
        assert!(plan_for("data:text/html,x", Scope::Origin).is_none());
    }

    #[test]
    fn malformed_dont_panic() {
        for s in ["", "://", "http://", "https://@/", "http://:80/", "https:///path"] {
            let _ = plan_for(s, Scope::Origin);
            let _ = plan_for(s, Scope::Domain);
        }
    }

    #[test]
    fn userinfo_stripped() {
        let p = plan_for("https://user:pass@example.com/x", Scope::Domain).unwrap();
        assert_eq!(p.target, "example.com");
    }

    #[test]
    fn http_and_https_both_supported() {
        assert!(plan_for("http://example.com/", Scope::Origin).is_some());
        assert!(plan_for("https://example.com/", Scope::Origin).is_some());
    }
}
