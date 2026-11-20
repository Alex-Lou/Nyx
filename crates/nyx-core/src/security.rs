//! Décision de navigation — couche de sécurité **pure**.
//!
//! Aucune dépendance GTK/WebKit ici : on prend une URL + le contexte de
//! confiance et on rend un verdict. `web/mod.rs` se contente d'exécuter ce
//! verdict (ignore/charge) — la *politique* vit ici, la *mécanique* là-bas.
//! (Sprint 4-5 : TLS, IDN/punycode, permissions viendront enrichir ce module,
//! candidat à devenir la crate `nyx-security`.)

use crate::nyxguard::NyxGuard;
use crate::sec_log::{self, Level};

/// Ce que la couche moteur doit faire d'une navigation.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Laisser WebKit poursuivre.
    Allow,
    /// Annuler la navigation, ne rien charger (pub bloquée, action non autorisée).
    Block,
    /// Annuler la navigation : tentative d'accès au système de fichiers refusée.
    /// Verdict distinct de `Block` pour permettre à l'UI d'afficher un message
    /// dédié ("Nyx ne laisse pas une page web ouvrir vos fichiers locaux").
    BlockFileAccess,
    /// Annuler la navigation et appliquer les réglages (`nyx://apply`).
    ApplySettings,
    /// Annuler la navigation et déplacer un favori (`nyx://move`).
    MoveBookmark,
    /// Annuler la navigation et effacer l'historique (`nyx://history/clear`).
    ClearHistory,
    /// Annuler la navigation et ouvrir l'assistant d'import Chrome
    /// (`nyx://import/chrome`). Lit des fichiers locaux : interne uniquement.
    ImportChrome,
    /// Annuler la navigation et proposer l'import du coffre de la capsule
    /// WSL (`nyx://import/capsule`, Windows). Lit un fichier local et demande
    /// une passphrase : interne uniquement.
    ImportCapsule,
    /// Annuler la navigation et ouvrir le dialogue natif du mot de passe
    /// principal (`nyx://master/set`, `nyx://master/remove`). Aucune
    /// passphrase ne transite par l'URL : elle est saisie dans l'UI native
    /// (dialogue GTK, panneau du chrome Windows). Interne uniquement.
    MasterPassword(MasterOp),
    /// Annuler la navigation et agir sur les appareils liés
    /// (`nyx://devices/pair|sync|rename?name=…|revoke?id=…`, Windows) :
    /// appairage, synchronisation, renommage, révocation (confirmée dans
    /// l'UI native). Interne uniquement ; la coquille analyse le reste.
    Devices,
    /// Annuler la navigation et charger une page interne.
    Load(Page),
}

impl Verdict {
    /// `true` pour les refus purs (aucune action ni chargement).
    pub fn is_block(&self) -> bool {
        matches!(self, Verdict::Block | Verdict::BlockFileAccess)
    }
}

/// Opération demandée sur le mot de passe principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasterOp {
    /// Définir (aucun mot de passe) ou changer (mot de passe existant).
    Set,
    /// Retirer : retour à la clé d'appareil.
    Remove,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Page { Settings, Bookmarks, History, NewTab }

/// Décide du sort d'une navigation.
///
/// `page_internal` indique si la page émettrice est de confiance
/// (`file://…/assets/` ou `nyx://`). C'est la frontière de sécurité : une
/// page web distante ne peut pas muter les réglages via `nyx://apply`.
pub fn decide(url: &str, page_internal: bool, guard: &NyxGuard) -> Verdict {
    let scheme = crate::url::scheme_of(url);
    if scheme.as_deref() == Some("nyx") {
        let rest = &url.trim()["nyx://".len()..];
        if rest.starts_with("apply") {
            return if page_internal { Verdict::ApplySettings } else {
                sec_log::emit(Level::Block, &format!("nyx://apply from remote page: {}", sec_log::redact_url(url)));
                Verdict::Block
            };
        }
        if rest.starts_with("move") {
            return if page_internal { Verdict::MoveBookmark } else {
                sec_log::emit(Level::Block, &format!("nyx://move from remote page: {}", sec_log::redact_url(url)));
                Verdict::Block
            };
        }
        if rest.starts_with("history/clear") {
            return if page_internal { Verdict::ClearHistory } else {
                sec_log::emit(Level::Block, &format!("nyx://history/clear from remote page: {}", sec_log::redact_url(url)));
                Verdict::Block
            };
        }
        if rest.starts_with("import/chrome") {
            return if page_internal { Verdict::ImportChrome } else {
                sec_log::emit(Level::Block, &format!("nyx://import from remote page: {}", sec_log::redact_url(url)));
                Verdict::Block
            };
        }
        if rest.starts_with("import/capsule") {
            return if page_internal { Verdict::ImportCapsule } else {
                sec_log::emit(Level::Block, &format!("nyx://import from remote page: {}", sec_log::redact_url(url)));
                Verdict::Block
            };
        }
        if let Some(op) = rest.strip_prefix("master/") {
            if !page_internal {
                sec_log::emit(Level::Block, &format!("nyx://master from remote page: {}", sec_log::redact_url(url)));
                return Verdict::Block;
            }
            if op.starts_with("set") { return Verdict::MasterPassword(MasterOp::Set); }
            if op.starts_with("remove") { return Verdict::MasterPassword(MasterOp::Remove); }
            return Verdict::Block;
        }
        if rest.starts_with("devices/") {
            return if page_internal { Verdict::Devices } else {
                sec_log::emit(Level::Block, &format!("nyx://devices from remote page: {}", sec_log::redact_url(url)));
                Verdict::Block
            };
        }
        if rest.starts_with("settings")  { return Verdict::Load(Page::Settings); }
        if rest.starts_with("bookmarks") { return Verdict::Load(Page::Bookmarks); }
        if rest.starts_with("history")   { return Verdict::Load(Page::History); }
        return Verdict::Load(Page::NewTab);
    }
    // file:// : autorisé UNIQUEMENT depuis une source interne (notre base_uri
    // `file://…/assets/` au démarrage, ou nyx://). Une page web distante ne
    // peut JAMAIS atteindre file://. Voir docs/security.md §2.
    if scheme.as_deref() == Some("file") && !page_internal {
        sec_log::emit(Level::Block, &format!("file:// navigation from remote: {}", sec_log::redact_url(url)));
        return Verdict::BlockFileAccess;
    }
    if guard.should_block(url) {
        sec_log::emit(Level::Block, &format!("ad/tracker: {}", sec_log::redact_url(url)));
        return Verdict::Block;
    }
    Verdict::Allow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> NyxGuard { NyxGuard::new() }

    #[test]
    fn uppercase_schemes_cannot_bypass_boundary() {
        assert_eq!(decide("FILE:///etc/passwd", false, &guard()), Verdict::BlockFileAccess);
        assert_eq!(decide("NYX://apply?adblock=false", false, &guard()), Verdict::Block);
        assert!(decide("Nyx://history/clear", false, &guard()).is_block());
    }

    #[test] fn allows_normal()       { assert_eq!(decide("https://example.com", true, &guard()), Verdict::Allow); }
    #[test] fn blocks_ad()           { assert_eq!(decide("https://doubleclick.net/x", true, &guard()), Verdict::Block); }
    #[test] fn settings_page()       { assert_eq!(decide("nyx://settings", false, &guard()), Verdict::Load(Page::Settings)); }
    #[test] fn bookmarks_page()      { assert_eq!(decide("nyx://bookmarks", false, &guard()), Verdict::Load(Page::Bookmarks)); }
    #[test] fn newtab_fallback()     { assert_eq!(decide("nyx://newtab", false, &guard()), Verdict::Load(Page::NewTab)); }
    #[test] fn unknown_nyx_to_newtab(){ assert_eq!(decide("nyx://wat", false, &guard()), Verdict::Load(Page::NewTab)); }

    #[test] fn apply_from_internal_ok() {
        assert_eq!(decide("nyx://apply?dark=true", true, &guard()), Verdict::ApplySettings);
    }
    #[test] fn apply_from_remote_blocked() {
        // SÉCURITÉ : une page distante ne peut pas muter les réglages.
        assert_eq!(decide("nyx://apply?adblock=false", false, &guard()), Verdict::Block);
    }
    #[test] fn move_from_internal_ok() {
        assert_eq!(decide("nyx://move?idx=0&to=Travail", true, &guard()), Verdict::MoveBookmark);
    }
    #[test] fn move_from_remote_blocked() {
        assert_eq!(decide("nyx://move?idx=0&to=evil", false, &guard()), Verdict::Block);
    }

    // ── Restriction file:// — aucune page web ne lit le disque ───────────
    #[test] fn blocks_file_from_remote() {
        assert_eq!(decide("file:///etc/passwd", false, &guard()), Verdict::BlockFileAccess);
    }
    #[test] fn allows_file_from_internal_for_assets() {
        // Au démarrage, le base_uri `file://…/assets/` doit charger : c'est
        // notre propre page interne. Le `page_internal=true` autorise.
        assert_eq!(decide("file:///mnt/c/Brol/Nyx/assets/", true, &guard()), Verdict::Allow);
    }
    #[test] fn blocks_ssh_keys_from_remote() {
        assert_eq!(decide("file:///home/user/.ssh/id_rsa", false, &guard()), Verdict::BlockFileAccess);
    }
    #[test] fn blocks_windows_path_from_remote() {
        assert_eq!(decide("file:///C:/Windows/System32/config", false, &guard()), Verdict::BlockFileAccess);
    }
    #[test] fn guard_off_allows_ad() {
        let g = guard();
        g.set_enabled(false);
        assert_eq!(decide("https://doubleclick.net/x", true, &g), Verdict::Allow);
    }

    #[test] fn history_page() {
        assert_eq!(decide("nyx://history", false, &guard()), Verdict::Load(Page::History));
    }
    #[test] fn clear_history_from_internal_ok() {
        assert_eq!(decide("nyx://history/clear", true, &guard()), Verdict::ClearHistory);
    }
    #[test] fn clear_history_from_remote_blocked() {
        // SÉCURITÉ : une page distante ne peut pas effacer l'historique.
        assert_eq!(decide("nyx://history/clear", false, &guard()), Verdict::Block);
    }

    #[test] fn import_chrome_from_internal_ok() {
        assert_eq!(decide("nyx://import/chrome", true, &guard()), Verdict::ImportChrome);
    }
    #[test] fn import_chrome_from_remote_blocked() {
        // SÉCURITÉ : une page distante ne peut pas déclencher la lecture des
        // fichiers Chrome ni ouvrir un sélecteur de fichiers.
        assert_eq!(decide("nyx://import/chrome", false, &guard()), Verdict::Block);
        assert!(decide("NYX://import/chrome", false, &guard()).is_block());
    }

    #[test] fn import_capsule_internal_only() {
        assert_eq!(decide("nyx://import/capsule", true, &guard()), Verdict::ImportCapsule);
        assert_eq!(decide("nyx://import/capsule", false, &guard()), Verdict::Block);
    }

    #[test]
    fn master_password_internal_only() {
        assert_eq!(decide("nyx://master/set", true, &guard()), Verdict::MasterPassword(MasterOp::Set));
        assert_eq!(decide("nyx://master/remove?t=1", true, &guard()), Verdict::MasterPassword(MasterOp::Remove));
        assert_eq!(decide("nyx://master/wat", true, &guard()), Verdict::Block);
        for op in ["nyx://master/set", "nyx://master/remove", "nyx://master/x"] {
            assert_eq!(decide(op, false, &guard()), Verdict::Block, "{op}");
        }
        assert!(decide("Nyx://import/capsule?x=1", false, &guard()).is_block());
    }

    #[test]
    fn devices_internal_only() {
        for op in ["nyx://devices/pair", "nyx://devices/revoke?id=ab", "nyx://devices/rename?name=x"] {
            assert_eq!(decide(op, true, &guard()), Verdict::Devices, "{op}");
            assert_eq!(decide(op, false, &guard()), Verdict::Block, "{op}");
        }
        // « nyx://devices » sans action : pas une action (page inconnue).
        assert_eq!(decide("nyx://devices", true, &guard()), Verdict::Load(Page::NewTab));
    }
}
