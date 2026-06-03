//! nyx-core — logique pure du navigateur Nyx.
//!
//! Aucune dépendance GTK/WebKit : état, sécurité, résolution d'URL.
//! Le compilateur enforce cette frontière (pas de `gtk` dans Cargo.toml).

pub mod domain_risk;
pub mod download_policy;
pub mod mode;
pub mod nyxguard;
pub mod permissions;
pub mod sec_log;
pub mod security;
pub mod site_data_policy;
pub mod state;
pub mod url;
pub mod vault_autofill;
