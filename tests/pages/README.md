# Pages hostiles — tests manuels

Pages HTML conçues pour **tester les défenses de Nyx en conditions réelles**.
Tu les ouvres dans Nyx (`file://` désormais bloqué : sers-les via un mini
serveur local — `python3 -m http.server 8000` dans ce dossier, puis
`http://localhost:8000/file_escape.html`).

Pour chaque page, le **comportement attendu** est documenté en tête.

## Liste

| Page                       | Tente d'attaquer                          | Attendu               |
|----------------------------|-------------------------------------------|-----------------------|
| `file_escape.html`         | Navigation vers `file:///etc/passwd`      | `BlockFileAccess`     |
| `popup_spam.html`          | `window.open()` en boucle                 | bloqué (setting déjà posé) |
| `clipboard_attack.html`    | `navigator.clipboard.readText()`          | refusé (WebKit setting) |
| `fake_login_iframe.html`   | iframe vers un faux paypal                | iframe chargée mais URL bar correcte ; iframe NyxGuard si dans liste |
| `download_exec.html`       | Force le DL d'un `.sh`                    | (à venir DownloadGuard) — pour l'instant DL standard |
| `idn_phishing.html`        | Lien vers `pаypal.com` (cyrillique)       | nav OK mais URL bar `nyx-risk-dangerous` |
| `storage_test.html`        | Cookie + localStorage + IDB               | persisté ; Forget this site doit tout effacer |
| `permission_camera.html`   | `getUserMedia({video:true})`              | (à venir PermissionManager) — actuellement WebKit prompt |
| `js_redirect_chain.html`   | Redirection JS vers file://, nyx://, data:| bloqué chaque tentative |
| `fingerprint_basics.html`  | canvas/font/timezone                      | exécuté (limites Sprint suivant) |

## Stress

`stress_traffic.html` : lance N requêtes parallèles + crée N onglets via
`window.open` (popup bloqué dans Nyx). Permet de voir le navigateur sous
charge.

## Comment ajouter une page

1. Une attaque = une page dédiée. Pas de mélange.
2. Bloc en tête : que tente l'attaque, comportement attendu, pourquoi.
3. Pas de dépendance externe (CSP `default-src 'none'` côté Nyx).
4. Si la page doit naviguer, utilise des URLs littérales — pas de raccourci.
