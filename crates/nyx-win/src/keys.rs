//! Raccourcis clavier globaux — mapping pur touche virtuelle → action.
//!
//! Les touches arrivent de l'événement WebView2 `AcceleratorKeyPressed`
//! (chrome ET contenu) : une page web ne peut ni les voir ni les simuler.

/// Action déclenchée par un raccourci.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    NewTab,
    CloseTab,
    FocusUrl,
    NextTab,
    PrevTab,
}

// Codes de touches virtuelles Win32 (`VK_*`).
const VK_TAB: u32 = 0x09;
const VK_PRIOR: u32 = 0x21; // Page préc.
const VK_NEXT: u32 = 0x22; // Page suiv.
const VK_F6: u32 = 0x75;
const VK_L: u32 = 0x4C;
const VK_T: u32 = 0x54;
const VK_W: u32 = 0x57;

/// Raccourci correspondant à `vk` avec l'état des modificateurs, s'il y en a.
/// Alt n'est jamais combiné ici (Alt+← / Alt+→ restent gérés par WebView2).
pub fn shortcut(vk: u32, ctrl: bool, shift: bool) -> Option<Shortcut> {
    match (vk, ctrl, shift) {
        (VK_T, true, false) => Some(Shortcut::NewTab),
        (VK_W, true, false) => Some(Shortcut::CloseTab),
        (VK_L, true, false) | (VK_F6, false, false) => Some(Shortcut::FocusUrl),
        (VK_TAB, true, false) | (VK_NEXT, true, false) => Some(Shortcut::NextTab),
        (VK_TAB, true, true) | (VK_PRIOR, true, false) => Some(Shortcut::PrevTab),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_shortcuts() {
        assert_eq!(shortcut(VK_T, true, false), Some(Shortcut::NewTab));
        assert_eq!(shortcut(VK_W, true, false), Some(Shortcut::CloseTab));
        assert_eq!(shortcut(VK_L, true, false), Some(Shortcut::FocusUrl));
        assert_eq!(shortcut(VK_F6, false, false), Some(Shortcut::FocusUrl));
        assert_eq!(shortcut(VK_TAB, true, false), Some(Shortcut::NextTab));
        assert_eq!(shortcut(VK_TAB, true, true), Some(Shortcut::PrevTab));
        assert_eq!(shortcut(VK_NEXT, true, false), Some(Shortcut::NextTab));
        assert_eq!(shortcut(VK_PRIOR, true, false), Some(Shortcut::PrevTab));
    }

    #[test]
    fn leaves_other_keys_to_the_page() {
        assert_eq!(shortcut(VK_T, false, false), None, "simple frappe");
        assert_eq!(shortcut(VK_T, true, true), None, "Ctrl+Maj+T : pas en phase 1");
        assert_eq!(shortcut(VK_TAB, false, false), None);
        assert_eq!(shortcut(0x43, true, false), None, "Ctrl+C");
    }
}
