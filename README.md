# Nyx

Navigateur léger et sûr par défaut : pubs et traqueurs bloqués (NyxGuard),
coffre chiffré pour les mots de passe, favoris et historique, téléchargements
vérifiés. Windows (WebView2), Android (WebView) et Linux (WebKitGTK), avec un
cœur Rust commun (`crates/nyx-core`).

| Dossier | Rôle |
|---|---|
| `crates/nyx-core` | Politique pure : URL, sécurité, anti-pub, téléchargements, thème |
| `crates/vault` | Coffre SQLCipher |
| `crates/nyx-win` | Appli Windows native (WebView2) |
| `crates/browser` | Appli Linux / capsule WSL (GTK + WebKitGTK) |
| `crates/nyx-ffi` + `android/` | Appli Android |
| `site/` | Page de téléchargement publique |

Publication : `.github/workflows/release.yml` (tag `vX.Y.Z`) publie `Nyx.exe`,
`Nyx.apk` et la page `site/` dans le dépôt public `nyx-releases`.

Commandes du quotidien : `docs/commandes.html`.

—
Signé Cybwu
