# Nyx — Sécurité

Document vivant. Décrit le modèle de menace, les politiques par surface, et
les principes que tout nouveau ticket doit respecter. À relire avant d'ajouter
une feature qui touche au système de fichiers, aux secrets, aux cookies, au
clipboard ou au réseau.

---

## 1. Modèle de menace

### Attaquants pris au sérieux
- **Site web malveillant** : page hostile ouverte dans un onglet.
- **Pub injectée** : ressource tierce qui exécute du JS arbitraire.
- **Fichier local piégé** : HTML local qui tente de lire d'autres fichiers.
- **Domaine phishing** : homographe (`pаypal.com`), TLD trompeur.
- **Extension future** : surface d'attaque énorme — non implémentée pour cette raison.
- **Malware déjà présent** : on ne peut pas tout — on limite la casse.

### Ce qu'ils veulent
- Lire des fichiers locaux (`/etc/passwd`, `~/.ssh/id_rsa`, vault).
- Voler mots de passe, cookies, tokens.
- Récupérer le clipboard en silence.
- Déclencher des téléchargements piégés.
- Contourner les permissions accordées par l'utilisateur.
- Fingerprint pour pister l'utilisateur entre sessions.

### Ce qu'on NE promet PAS
- Ne pas dire « inviolable », « 100% anonyme », « bloque tous les malwares ».
- Promesses honnêtes : réduit les risques, isole les profils, garde le vault
  local, bloque beaucoup d'intrusions connues.

---

## 2. Politiques par surface

Pour chaque surface : **autorisé quand**, **bloqué quand**, **demande user quand**,
**loggé comment**, **testé comment**.

### `file://`

| Source                      | Action | Code |
|-----------------------------|--------|------|
| Page web distante           | **Bloqué** (`Verdict::BlockFileAccess`) | `nyx-core::security::decide` |
| Page interne `nyx://`       | **Bloqué** — aucune page interne n'en a besoin | idem |
| URL bar utilisateur         | **Bloqué pour cette itération**, à venir : confirmation | idem |
| Menu « Ouvrir un fichier »  | Non implémenté — à venir avec scope contrôlé | — |
| Resources des pages internes (assets) | Allow via `ResourceLoad` (pas `NavigationAction`) | WebKit interne |

**Logué** : `[BLOCK] file:// navigation from <origin>`
**Testé** : `nyx-core::security::tests::blocks_file_*` + page hostile.

### `nyx://`

| URI                | Page interne ?  | Verdict |
|--------------------|-----------------|---------|
| `nyx://newtab`     | toujours autorisé | `Load(NewTab)` |
| `nyx://settings`   | toujours autorisé | `Load(Settings)` |
| `nyx://bookmarks`  | toujours autorisé | `Load(Bookmarks)` |
| `nyx://apply?…`    | Page interne uniquement | `ApplySettings` |
| `nyx://move?…`     | Page interne uniquement | `MoveBookmark` |
| `nyx://master/set`, `nyx://master/remove` | Page interne uniquement | `MasterPassword(op)` : ouvre la saisie native, aucune passphrase dans l'URL |
| `nyx://*` inconnu  | tombe sur `NewTab` (jamais d'exécution) | `Load(NewTab)` |

**Loggué** : `[BLOCK] nyx://apply from remote page` quand `page_internal = false`.
**Testé** : `security::tests::apply_from_remote_blocked`, `move_from_remote_blocked`.

### Downloads (à implémenter — DownloadGuard)

- Types neutres (image, vidéo, audio, pdf, texte) → Allow silencieux.
- Types exécutables (`.exe`, `.msi`, `.deb`, `.dmg`, `.AppImage`, `.desktop`,
  `.sh`, `.bat`, `.app`) → demander confirmation explicite, afficher origine.
- Originaire d'une page Dangerous (IDN suspect) → bloquer par défaut.
- En mode Shadow → demander à chaque fois, jamais de mémoire « ne plus demander ».
- En mode Banking → bloquer sauf whitelist explicite.

**Loggué** : `[ASK] download .desktop from <origin>` / `[DENY] download from suspicious origin`.

### Coffre : clé d'appareil ou mot de passe principal

Le coffre (`vault.db`) est **toujours** chiffré par SQLCipher (AES-256).
Le mot de passe principal est **optionnel** (`vault::keymode`) :

| Mode | Clé SQLCipher | Où elle vit | Protège contre | Ne protège PAS contre |
|------|---------------|-------------|----------------|-----------------------|
| Clé d'appareil (défaut) | 32 octets CSPRNG, clé brute `x'…'` | `vault.key` à côté du coffre : Windows = scellé DPAPI (utilisateur courant) ; Linux = fichier 0600, dossier 0700 | vol du disque / du fichier seul (Windows), autres comptes | quiconque agit dans la **session ouverte** (malware, accès physique déverrouillé) — même modèle que Chrome/Firefox sans mot de passe principal |
| Mot de passe principal | dérivée par PBKDF2 (SQLCipher) | nulle part | aussi la session ouverte tant que Nyx est fermé | keylogger dans la session ; oubli = données perdues |

Règles :

- Démarrage : `vault.key` présent → ouverture silencieuse ; s'il échoue
  (corrompu, autre compte) → demande du mot de passe principal avec message,
  **rien n'est supprimé automatiquement**. Coffre sans `vault.key` → mot de
  passe principal (coffres existants inchangés). Pas de coffre → bienvenue.
- Transitions (`PRAGMA rekey`) toujours vérifiées en rouvrant le coffre avec
  la nouvelle clé ; échec → état précédent restauré. `vault.key` est écrit
  (atomique : temporaire + fsync + rename) et relu **avant** de rechiffrer
  vers une clé d'appareil, et supprimé **après** un rechiffrement vérifié
  vers un mot de passe.
- Clés jamais journalisées, `Debug` masqué, octets effacés à la destruction
  (au mieux : les copies internes de rusqlite/SQLCipher/WebView échappent à
  ce contrôle). Passphrases saisies uniquement dans l'UI native (dialogue
  GTK, chrome `nyxui` sous Windows), jamais dans une URL `nyx://`.
- Linux : sans chiffrement de `vault.key` au repos, la protection repose sur
  les droits Unix et le chiffrement du disque. **Amélioration future** :
  sceller la clé dans le trousseau Secret Service (GNOME Keyring / KWallet).

**Testé** : `vault::keymode::tests` (détection, cycles complets, repli sans
suppression, abandon si le fichier de clé est illisible), `vault::keyfile::tests`
(atomicité, droits 0600/0700, en-tête), `vault::tests::rekey_round_trips_raw_and_passphrase`,
`security::tests::master_password_internal_only`, `nyx-win::dpapi::tests` (Windows).

### Vault & autofill (à implémenter — VaultAutofillPolicy)

- Autofill **jamais automatique** par défaut. L'utilisateur choisit toujours.
- Domaine `Dangerous` (homographe, mixed-script) → **autofill refusé même si l'utilisateur clique**, avec message explicatif.
- Domaine `Suspicious` (IDN propre) → autofill possible mais avertissement clair.
- HTTP non-TLS → **autofill refusé** (downgrade attack).
- Origin mismatch (formulaire pointe ailleurs) → refusé.

**Loggué** : `[DENY] vault autofill: domain mismatch` / `[ALLOW] vault autofill: paypal.com`.

### Clipboard

- Lecture JS bloquée par défaut (WebKit setting déjà posé).
- Écriture JS autorisée mais limitée par WebKit (gesture-gated).
- Mode Banking : lecture **et** écriture désactivées.

### Permissions (caméra, micro, géoloc, notifs)

- Par défaut : **Ask** à chaque demande, mémoire optionnelle par origine.
- Mode Shadow : **toujours Ask**, jamais de mémoire.
- Mode Banking : **toujours Deny** sauf whitelist explicite.
- Une permission accordée est **liée à l'origine canonique** (HTTPS + host + port par défaut retiré).
- « Forget this site » révoque toutes les permissions du domaine.

### Cookies & storage

- Par défaut : persistants (mode Normal), session-only (Shadow), strict (Banking).
- Mode privé (`private_mode` dans `AppSettings`) : `WebContext::new_ephemeral()` → rien sur disque.
- « Forget this site » efface cookies + localStorage + IndexedDB + cache + service workers (`site_data_policy::Plan`).

### IDN / homographes

- Tous les hosts http(s) passent dans `domain_risk::analyze`.
- `Safe` : silencieux.
- `Suspicious` (IDN propre, ex: `bücher.de`) : URL bar légère teinte twilight + tooltip ASCII.
- `Dangerous` (mixed-script ou ressemble à un site connu) : URL bar rouge, refus d'autofill vault.

---

## 3. Defaults stricts

L'application démarre **strict**. L'utilisateur peut ouvrir des droits ; jamais l'inverse.

- Bloquer plutôt qu'autoriser.
- Ask plutôt que deviner.
- Session-only plutôt que persistant en cas de doute.
- Pas de télémétrie.
- Pas d'auto-fill automatique.
- Pas de clipboard libre.
- Pas de suggestions remote (search suggestions désactivées).

---

## 4. Logging sécurité

Activable via `NYX_SECURITY_DEBUG=1`.

Format :
```
[BLOCK] file:// navigation from https://evil.test
[WARN]  suspicious IDN: xn--paypl-fve.com (paypаl.com)
[DENY]  clipboard read from https://example.com
[ALLOW] download image/png from https://example.com
[DENY]  vault autofill: domain mismatch (form → evil.com, vault → paypal.com)
```

Stocké dans `nyx-core::sec_log`. Pure : pas d'I/O en prod, juste un format
déterministe. Le browser branche stderr.

### Ne JAMAIS logger
- Mots de passe.
- Cookies (valeur ; le nom du cookie est OK).
- Tokens, headers `Authorization`.
- Query params sensibles (`?token=...`, `?password=...`).
- Contenu de formulaires.
- Contenu du clipboard.

Pour les URLs sensibles, redact : `https://example.com/path?[redacted]`.

---

## 5. Tests obligatoires

Toute nouvelle feature qui touche la sécurité doit livrer :
- **Tests fonctionnels** : le bon comportement dans le cas nominal.
- **Tests hostiles** : un attaquant ne peut pas contourner.
- **Tests panic** : entrées malformées massives, ne paniquent jamais.
- **Tests de stress** : milliers d'itérations sur entrées variées (déjà
  appliqué dans `nyxguard`, `settings`, `domain_risk`).

Pages de test hostiles : `tests/pages/` (servies via WebKit pour valider
end-to-end). Voir `tests/pages/README.md`.

---

## 6. Modes

| Mode    | Cookies     | Blocker | Vault       | Clipboard | Permissions |
|---------|-------------|---------|-------------|-----------|-------------|
| Normal  | persistants | normal  | manuel      | gesture   | Ask + mémoire |
| Shadow  | session     | strict  | lecture only| gesture   | Ask, jamais mémoire |
| Banking | session     | strict  | très strict | bloqué    | Deny sauf whitelist |
| Dev     | session     | off     | manuel      | libre     | toutes Allow |

À implémenter : Sprint 5 (`feat/profiles`).

---

## 7. Updates & dépendances

- `Cargo.lock` commité.
- `cargo audit` à chaque PR (CI à venir).
- WebKit à jour (suivre `webkit2gtk` upstream).
- Changelog upstream lu avant bump majeur.

---

## 8. Reset

L'utilisateur doit pouvoir tout effacer en un endroit :
- Reset cookies + storage globalement.
- Reset permissions.
- Reset vault local.
- Reset blocker rules custom.
- Reset settings → defaults.

À implémenter dans `settings.html` (Sprint 5).

---

## 9. La règle finale

Avant tout merge d'une feature, répondre :

1. Est-ce qu'un site web peut l'abuser ?
2. Est-ce que ça touche fichiers, mots de passe, cookies, clipboard ?
3. Est-ce que ça persiste quelque chose ?
4. Est-ce que ça doit différer en Shadow / Banking ?
5. Y a-t-il un test hostile dédié ?
6. Y a-t-il un test panic (entrées malformées) ?
7. Y a-t-il un test de stress (milliers d'itérations) ?

Si une réponse est non, le ticket n'est pas prêt.
