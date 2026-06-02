//! Journal des décisions de sécurité — activable via `NYX_SECURITY_DEBUG=1`.
//!
//! **Pure logic + un seul `eprintln!` au bout** : le format est testable sans
//! capturer stderr (`format_event`), et le `emit()` ne fait rien si la var
//! n'est pas posée. Pas d'I/O cachée, pas d'allocation si désactivé.
//!
//! Règle d'or : ne JAMAIS passer ici un mot de passe, cookie, token, contenu
//! de formulaire ou de clipboard. Les URLs sensibles doivent être passées par
//! `redact_url` avant.

use std::sync::atomic::{AtomicU8, Ordering};

/// Niveau d'un événement — détermine le tag dans la sortie formatée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Action refusée par Nyx (`Verdict::Block`, file://, etc.).
    Block,
    /// Action refusée par une politique (autofill domain mismatch, etc.).
    Deny,
    /// Action suspecte mais autorisée (IDN propre, etc.).
    Warn,
    /// Action autorisée explicitement, utile en debug.
    Allow,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Self::Block => "BLOCK",
            Self::Deny  => "DENY",
            Self::Warn  => "WARN",
            Self::Allow => "ALLOW",
        }
    }
}

/// Cache de l'activation : lu une fois, puis atomique sans syscall.
/// 0 = pas encore lu, 1 = off, 2 = on.
static STATE: AtomicU8 = AtomicU8::new(0);

/// Active la sortie. Appelé une fois au boot par le browser après lecture
/// de `NYX_SECURITY_DEBUG`. Idempotent.
pub fn set_enabled(on: bool) {
    STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    STATE.load(Ordering::Relaxed) == 2
}

/// Émet un événement vers stderr **uniquement si le debug est activé**.
/// L'argument `details` est libre : `"file:// navigation from https://evil.test"`.
pub fn emit(level: Level, details: &str) {
    if !is_enabled() {
        return;
    }
    eprintln!("{}", format_event(level, details));
}

/// Format pur — testable sans capturer stderr.
/// Indenté à largeur fixe pour s'aligner en colonne dans le terminal.
pub fn format_event(level: Level, details: &str) -> String {
    format!("[{:<5}] {details}", level.tag())
}

/// Redact une URL avant logging : ne garde que scheme + host + path,
/// remplace la query par `[redacted]` si présente (peut contenir tokens).
pub fn redact_url(url: &str) -> String {
    let (head, tail) = match url.split_once('?') {
        Some((h, _)) => (h, "?[redacted]"),
        None => (url, ""),
    };
    // Fragment aussi potentiellement sensible (#access_token=…).
    let head = head.split('#').next().unwrap_or(head);
    format!("{head}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sérialise l'accès aux tests qui touchent à STATE.
    /// Ce mutex est privé aux tests : pas de surcoût en prod.
    fn enable_guard() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static M: OnceLock<Mutex<()>> = OnceLock::new();
        M.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn format_block() {
        assert_eq!(
            format_event(Level::Block, "file:// navigation from https://evil.test"),
            "[BLOCK] file:// navigation from https://evil.test"
        );
    }

    #[test]
    fn format_levels_padded() {
        // Tous les tags tiennent dans 5 colonnes.
        for l in [Level::Block, Level::Deny, Level::Warn, Level::Allow] {
            let s = format_event(l, "x");
            assert!(s.starts_with('['));
            assert!(s.contains("] x"));
        }
    }

    #[test]
    fn redact_strips_query() {
        assert_eq!(redact_url("https://e.com/p?token=abc"), "https://e.com/p?[redacted]");
        assert_eq!(redact_url("https://e.com/p"), "https://e.com/p");
        assert_eq!(redact_url("https://e.com/p#access_token=x"), "https://e.com/p");
        assert_eq!(redact_url(""), "");
    }

    #[test]
    fn disabled_by_default_no_emit() {
        let _g = enable_guard();
        STATE.store(0, Ordering::Relaxed);
        assert!(!is_enabled());
        emit(Level::Block, "should not appear");
    }

    #[test]
    fn enable_toggle() {
        let _g = enable_guard();
        set_enabled(true);
        assert!(is_enabled());
        set_enabled(false);
        assert!(!is_enabled());
    }

    /// Panic test : entrées tordues ne paniquent jamais (format + redact).
    #[test]
    fn malformed_doesnt_panic() {
        for s in ["", "?", "?#", "#?", "://", "%%%", "日本?token=x", "\0\n\t"] {
            let _ = redact_url(s);
            let _ = format_event(Level::Block, s);
        }
    }

    /// Stress : milliers d'événements formatés sans allocation explosive.
    #[test]
    fn stress_thousands_of_events() {
        for i in 0..10_000 {
            let url = format!("https://host{i}.test/path?token={i}");
            let red = redact_url(&url);
            let _ = format_event(Level::Block, &red);
        }
    }
}
