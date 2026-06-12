//! Couche interface GTK : fenêtre, barre de navigation, onglets, raccourcis,
//! thème. Consomme `crate::web` (moteur) et `crate::state` (réglages/favoris).

pub mod anim;
pub mod bookmarks_popover;
pub mod chrome;
pub mod dialog;
pub mod downloads;
pub mod icon;
pub mod navbar;
pub mod settings_window;
pub mod shortcuts;
pub mod tabs;
pub mod theme;
pub mod toast;
pub mod window;
