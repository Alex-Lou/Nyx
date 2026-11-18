//! Import des données Chrome — parsers purs (aucun GTK, aucune base de données).
//!
//! - `csv` : parseur CSV minimal (RFC 4180) ;
//! - `chrome` : favoris JSON, timestamps, découverte des profils ;
//! - `passwords` : export CSV du gestionnaire de mots de passe Chrome.
//!
//! Le HTML Netscape est déjà géré par `state::bookmarks::import_netscape`.

pub mod chrome;
pub mod csv;
pub mod passwords;
