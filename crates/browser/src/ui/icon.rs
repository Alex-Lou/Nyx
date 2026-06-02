use gtk::gdk_pixbuf::Pixbuf;
use gtk::Window;

/// Logo Nyx embarqué (croissant + vague).
const LOGO: &[u8] = include_bytes!("../../../../assets/icons/nyx.png");

/// Définit l'icône par défaut de **toutes** les fenêtres Nyx — titlebar,
/// barre des tâches, alt-tab, et l'icône de l'app au sens large.
pub fn set_default() {
    if let Ok(pb) = Pixbuf::from_read(std::io::Cursor::new(LOGO)) {
        Window::set_default_icon(&pb);
    }
}
