//! nyx-core — logique pure du navigateur Nyx.
//!
//! Aucune dépendance GTK/WebKit : état, sécurité, résolution d'URL.
//! Le compilateur enforce cette frontière (pas de `gtk` dans Cargo.toml).

pub mod domain_risk;
pub mod nyxguard;
pub mod security;
pub mod site_data_policy;
pub mod state;
pub mod url;
