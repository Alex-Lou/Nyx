//! Preload list — domaines qui sont **toujours** HTTPS, peu importe ce que
//! le serveur a envoyé comme header. Subset curé de la liste Chrome.
//!
//! Politique : toute entrée ici est traitée comme `includeSubdomains=true`,
//! `expires_at_unix=+∞`. La liste est volontairement **petite et notoire**
//! (~50 entrées) : moteurs de recherche, plateformes cloud, banques majeures,
//! gestionnaires de mots de passe. Pour étendre, vérifier sur
//! <https://hstspreload.org>.
//!
//! Ce qu'elle ne fait PAS : remplacer la PSL Chrome complète. Le browser
//! peut compléter via `upsert_from_header` côté `HstsStore`.

const PRELOAD_HOSTS: &[&str] = &[
    // Moteurs / régies
    "google.com", "googleapis.com", "gstatic.com",
    "youtube.com", "youtubekids.com",
    "duckduckgo.com",
    // Réseaux sociaux
    "twitter.com", "x.com",
    "facebook.com", "instagram.com", "whatsapp.com",
    "linkedin.com", "reddit.com", "discord.com",
    // Code / dev
    "github.com", "gitlab.com", "bitbucket.org",
    "rust-lang.org", "crates.io",
    "python.org", "pypi.org",
    "npmjs.com",
    // Cloud / CDN
    "cloudflare.com", "fastly.com",
    "amazon.com", "amazonaws.com",
    "microsoft.com", "azure.com",
    "apple.com", "icloud.com",
    "mozilla.org",
    // Paiement / banque
    "paypal.com", "stripe.com",
    "chase.com", "bankofamerica.com", "wellsfargo.com",
    "americanexpress.com", "citi.com",
    "revolut.com", "n26.com", "monzo.com",
    "bnpparibas.fr", "societegenerale.fr",
    "credit-agricole.fr",
    // Identité / vault
    "bitwarden.com", "1password.com",
    "lastpass.com", "dashlane.com",
    "protonmail.com", "proton.me",
    // Divers (gros traffic HTTPS-only)
    "wikipedia.org", "wikimedia.org",
    "dropbox.com", "slack.com", "zoom.us",
    "stackoverflow.com",
];

/// `true` si `host` (ou un de ses parents) est dans la preload list.
/// Toute entrée est traitée comme `includeSubdomains`.
pub fn is_preloaded(host: &str) -> bool {
    if host.is_empty() { return false; }
    if PRELOAD_HOSTS.contains(&host) { return true; }
    let mut s = host;
    while let Some(dot) = s.find('.') {
        s = &s[dot + 1..];
        if PRELOAD_HOSTS.contains(&s) { return true; }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        assert!(is_preloaded("github.com"));
        assert!(is_preloaded("paypal.com"));
        assert!(is_preloaded("rust-lang.org"));
    }

    #[test]
    fn subdomain_match() {
        assert!(is_preloaded("api.github.com"));
        assert!(is_preloaded("login.paypal.com"));
        assert!(is_preloaded("docs.rust-lang.org"));
        assert!(is_preloaded("a.b.c.cloudflare.com"));
    }

    #[test]
    fn injection_attempt_rejected() {
        // "github.com.evil.tld" ne doit PAS matcher github.com.
        assert!(!is_preloaded("github.com.evil.tld"));
        assert!(!is_preloaded("notgithub.com"));
        assert!(!is_preloaded(""));
        assert!(!is_preloaded("example.com"));
    }

    #[test]
    fn at_least_fifty_entries() {
        // Garde-fou : si quelqu'un éclate la liste, on s'en rend compte.
        assert!(PRELOAD_HOSTS.len() >= 50,
            "preload list shrunk to {}", PRELOAD_HOSTS.len());
    }
}
