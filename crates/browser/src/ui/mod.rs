//! Couche interface GTK : fenêtre, barre de navigation, onglets, raccourcis,
//! thème. Consomme `crate::web` (moteur) et `crate::state` (réglages/favoris).

pub mod bookmarks_popover;
pub mod chrome;
pub mod icon;
pub mod navbar;
pub mod settings_window;
pub mod shortcuts;
pub mod tabs;
pub mod theme;
pub mod window;
