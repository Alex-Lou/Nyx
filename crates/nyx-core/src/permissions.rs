//! PermissionManager central — caméra, micro, géoloc, notifications, clipboard.
//!
//! **Politique pure** (sans WebKit, sans I/O). Le browser :
//! 1. Reçoit la demande WebKit (`PermissionRequest`).
//! 2. Convertit en `Permission` + extrait l'origine.
//! 3. Appelle `Store::decide(origin, perm)` → `Decision`.
//! 4. Si `Ask`, affiche un prompt utilisateur, puis stocke via `grant()` / `deny()`.
//!
//! Tout est origine-scopé (RFC 6454 — scheme + host + port canoniques).
//! Une décision pour `https://example.com` ne fuit pas vers `http://...`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::mode::Mode;
use crate::sec_log::{self, Level};

/// Surface à laquelle un site demande accès.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    Camera,
    Microphone,
    Geolocation,
    Notifications,
    /// Lecture du presse-papiers (l'écriture est gesture-gated par WebKit).
    ClipboardRead,
    /// MIDI avec messages SysEx (surface d'attaque réelle sur certains devices).
    MidiSysex,
}

impl Permission {
    fn name(self) -> &'static str {
        match self {
            Self::Camera        => "camera",
            Self::Microphone    => "microphone",
            Self::Geolocation   => "geolocation",
            Self::Notifications => "notifications",
            Self::ClipboardRead => "clipboard_read",
            Self::MidiSysex     => "midi_sysex",
        }
    }
}

/// Résultat d'une demande.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Autorisé sans demander.
    Allow,
    /// Refusé sans demander.
    Deny,
    /// Demander à l'utilisateur (puis appeler `grant`/`deny` selon sa réponse).
    Ask,
}

/// Store partagé (Rc<RefCell<…>> — le navigateur est single-threaded GTK).
pub type PermissionStore = Rc<RefCell<Store>>;

#[derive(Default)]
pub struct Store {
    /// Décisions persistées : `(origin, permission) → Allow|Deny`.
    /// On ne stocke jamais `Ask` (c'est le défaut implicite).
    saved: HashMap<(String, Permission), Decision>,
    mode:  Mode,
}

pub fn new() -> PermissionStore {
    Rc::new(RefCell::new(Store::default()))
}

impl Store {
    pub fn set_mode(&mut self, m: Mode) { self.mode = m; }
    pub fn mode(&self) -> Mode { self.mode }

    /// Décide sans muter l'état. L'appelant doit appeler `grant`/`deny` ensuite
    /// si la décision était `Ask` et que l'utilisateur a répondu.
    pub fn decide(&self, origin: &str, perm: Permission) -> Decision {
        // Origine vide ou non-HTTPS → refus sec, jamais d'accès périphérique.
        if !is_trustworthy(origin) {
            sec_log::emit(Level::Deny,
                &format!("{} on non-secure origin: {origin}", perm.name()));
            return Decision::Deny;
        }
        match self.mode {
            Mode::Banking => self.saved
                .get(&(origin.to_string(), perm))
                .copied()
                .unwrap_or(Decision::Deny),
            Mode::Shadow => Decision::Ask, // jamais de mémoire
            // Dev se comporte comme Normal pour les permissions : aucune
            // surface d'attaque ne justifie d'autoriser caméra/micro plus
            // largement en local que sur le web public.
            Mode::Normal | Mode::Dev => self.saved
                .get(&(origin.to_string(), perm))
                .copied()
                .unwrap_or(Decision::Ask),
        }
    }

    /// Mémorise une autorisation (jamais en mode Shadow).
    pub fn grant(&mut self, origin: String, perm: Permission) {
        if self.mode == Mode::Shadow { return; }
        sec_log::emit(Level::Allow, &format!("{} on {origin}", perm.name()));
        self.saved.insert((origin, perm), Decision::Allow);
    }

    /// Mémorise un refus (jamais en mode Shadow).
    pub fn deny(&mut self, origin: String, perm: Permission) {
        if self.mode == Mode::Shadow { return; }
        sec_log::emit(Level::Deny, &format!("{} on {origin}", perm.name()));
        self.saved.insert((origin, perm), Decision::Deny);
    }

    /// Retire toute mémoire pour cette origine (appelé par « Forget this site »).
    pub fn revoke_origin(&mut self, origin: &str) {
        self.saved.retain(|(o, _), _| o != origin);
    }

    /// Retire tout (Reset Nyx).
    pub fn reset(&mut self) {
        self.saved.clear();
    }

    pub fn len(&self) -> usize { self.saved.len() }
    pub fn is_empty(&self) -> bool { self.saved.is_empty() }
}

/// HTTPS ou origine locale (localhost / 127.0.0.1) → trustworthy.
/// Tout le reste → refus dur. Aligné sur le W3C Secure Contexts spec.
fn is_trustworthy(origin: &str) -> bool {
    if let Some(rest) = origin.strip_prefix("https://") {
        return !rest.is_empty();
    }
    if let Some(rest) = origin.strip_prefix("http://") {
        return rest.starts_with("localhost")
            || rest.starts_with("127.")
            || rest.starts_with("[::1]");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s() -> Store { Store::default() }

    #[test]
    fn default_is_ask_in_normal_mode() {
        let s = s();
        assert_eq!(s.decide("https://example.com", Permission::Camera), Decision::Ask);
    }

    #[test]
    fn grant_then_decide_returns_allow() {
        let mut s = s();
        s.grant("https://example.com".into(), Permission::Camera);
        assert_eq!(s.decide("https://example.com", Permission::Camera), Decision::Allow);
    }

    #[test]
    fn deny_then_decide_returns_deny() {
        let mut s = s();
        s.deny("https://example.com".into(), Permission::Microphone);
        assert_eq!(s.decide("https://example.com", Permission::Microphone), Decision::Deny);
    }

    #[test]
    fn http_is_denied_no_matter_what() {
        let mut s = s();
        s.grant("http://example.com".into(), Permission::Camera);
        // Le grant a été stocké, mais decide refuse parce que non-secure.
        assert_eq!(s.decide("http://example.com", Permission::Camera), Decision::Deny);
    }

    #[test]
    fn localhost_http_is_trustworthy() {
        let s = s();
        assert_eq!(s.decide("http://localhost:3000", Permission::Camera), Decision::Ask);
        assert_eq!(s.decide("http://127.0.0.1:8080", Permission::Microphone), Decision::Ask);
    }

    #[test]
    fn shadow_mode_never_remembers() {
        let mut s = s();
        s.set_mode(Mode::Shadow);
        s.grant("https://example.com".into(), Permission::Camera);
        assert!(s.is_empty());
        assert_eq!(s.decide("https://example.com", Permission::Camera), Decision::Ask);
    }

    #[test]
    fn banking_default_is_deny() {
        let mut s = s();
        s.set_mode(Mode::Banking);
        assert_eq!(s.decide("https://example.com", Permission::Camera), Decision::Deny);
    }

    /// Dev mode (partagé avec download_policy) doit se comporter comme Normal :
    /// pas de relâchement sur caméra/micro/géoloc en local.
    #[test]
    fn dev_behaves_like_normal_for_permissions() {
        let mut s = s();
        s.set_mode(Mode::Dev);
        assert_eq!(s.decide("https://example.com", Permission::Camera), Decision::Ask);
        s.grant("https://example.com".into(), Permission::Camera);
        assert_eq!(s.decide("https://example.com", Permission::Camera), Decision::Allow);
        // Dev mémorise comme Normal (contrairement à Shadow).
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn banking_explicit_grant_works() {
        let mut s = s();
        s.set_mode(Mode::Banking);
        s.grant("https://bank.com".into(), Permission::Notifications);
        assert_eq!(s.decide("https://bank.com", Permission::Notifications), Decision::Allow);
    }

    #[test]
    fn origin_scope_doesnt_leak() {
        let mut s = s();
        s.grant("https://example.com".into(), Permission::Camera);
        // sub.example.com est une autre origine → pas d'héritage.
        assert_eq!(s.decide("https://sub.example.com", Permission::Camera), Decision::Ask);
        // Même origin, autre permission → indépendant.
        assert_eq!(s.decide("https://example.com", Permission::Microphone), Decision::Ask);
    }

    #[test]
    fn revoke_origin_clears_all_perms() {
        let mut s = s();
        s.grant("https://example.com".into(), Permission::Camera);
        s.grant("https://example.com".into(), Permission::Microphone);
        s.grant("https://other.com".into(),  Permission::Camera);
        s.revoke_origin("https://example.com");
        assert_eq!(s.decide("https://example.com", Permission::Camera), Decision::Ask);
        assert_eq!(s.decide("https://example.com", Permission::Microphone), Decision::Ask);
        // Other.com intact.
        assert_eq!(s.decide("https://other.com", Permission::Camera), Decision::Allow);
    }

    #[test]
    fn malformed_origins_dont_panic() {
        let s = s();
        for o in ["", "://", "https://", "http://", "ftp://x", "javascript:x", "ws://", "\0\n"] {
            assert_eq!(s.decide(o, Permission::Camera), Decision::Deny);
        }
    }

    /// Stress : 10 000 demandes variées — l'état reste cohérent et le store
    /// ne grossit pas hors limite (1 entrée par (origin, perm) unique).
    #[test]
    fn stress_thousands_of_requests() {
        let mut s = s();
        for i in 0..10_000 {
            let origin = format!("https://site{}.test", i % 50);
            let perm = match i % 4 {
                0 => Permission::Camera,
                1 => Permission::Microphone,
                2 => Permission::Geolocation,
                _ => Permission::Notifications,
            };
            if i % 3 == 0 { s.grant(origin.clone(), perm); }
            else if i % 3 == 1 { s.deny(origin.clone(), perm); }
            let _ = s.decide(&origin, perm);
        }
        // 50 origins × 4 permissions = 200 entrées max.
        assert!(s.len() <= 200);
    }
}
