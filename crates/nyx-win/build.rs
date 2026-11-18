//! Embarque l'icône et les métadonnées dans `nyx-win.exe` — uniquement
//! quand on compile POUR Windows depuis Windows (rc.exe / windres requis).
//! Ailleurs (Linux, vérification croisée), ne fait rien.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/icons/nyx.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icons/nyx.ico")
            .set("ProductName", "Nyx")
            .set("FileDescription", "Nyx")
            .set("LegalCopyright", "Nyx");
        if let Err(err) = res.compile() {
            // Pas d'icône plutôt qu'un build cassé (SDK Windows absent…).
            println!("cargo:warning=ressources Windows non embarquées : {err}");
        }
    }
}
