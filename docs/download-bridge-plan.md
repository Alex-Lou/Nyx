# Tâche 5 — Bridge WebKit du DownloadGuard

> **Statut** : plan en attente de validation. Aucune ligne de code côté
> `crates/browser/src/web/` ni `crates/browser/src/state/` tant que le user
> n'a pas explicitement validé ce document.

Objectif : brancher les signaux WebKit de téléchargement sur le
`DownloadStore` + `download_policy` + `magic_bytes` déjà en place, sans
introduire de breach (sécurité, GTK, architecture).

---

## 1. Frontière architecturale (verrou compilateur)

```
WebKit (signal "download-started")
        ↓
crates/browser/src/web/downloads/        ← I/O, GTK, WebKit
        ↓
crates/nyx-core/src/downloads/           ← pure (state + policy)
        ↓ (consomme)
download_policy / magic_bytes / sec_log  ← déjà existant
```

- `nyx-core/src/downloads/quarantine.rs` (nouveau, pur) : sérialisation
  `.nyxmeta`, parsing, helpers.
- `nyx-core/src/downloads/staging.rs` (nouveau, pur) : build du temp path,
  random suffix, sanity-check du final path.
- `crates/browser/src/web/downloads/` (nouveau, GTK/WebKit) : 5 fichiers
  petits qui orchestrent l'I/O.

**Garantie compilateur** : `nyx-core/Cargo.toml` reste sans `gtk` /
`webkit2gtk`. Le seul ajout de dépendance pure : `sha2 = "0.10"` (Rust
pur, RustCrypto, audité), pour calculer le SHA-256 dans nyx-core sans
sortir de la pureté.

---

## 2. Signaux WebKit utilisés

Sur le `WebContext` (singleton ou par-onglet selon la config existante,
à confirmer en lecture de `crates/browser/src/web/mod.rs`) :

| Signal               | Cible           | Action                                  |
|----------------------|-----------------|-----------------------------------------|
| `download-started`   | `WebContext`    | crée l'entrée store, connecte la suite  |
| `decide-destination` | `Download`      | fournit le **temp path** (pas le final) |
| `received-data`      | `Download`      | update progress (received + total)      |
| `finished`           | `Download`      | déclenche le post-flight (verdict)      |
| `failed`             | `Download`      | mark_failed + delete temp               |

Signal **explicitement ignoré** : `created-destination` — on ne fait rien
ici, la décision d'écrire le path final est post-finished, pas pré.

Pas de hook sur `decide-policy` côté navigation : ça reste géré par le
filter de navigation existant. Le bridge ne change pas le routing nyx://.

---

## 3. Pipeline du download (étape par étape)

### Phase A — Démarrage (signal `download-started`)

1. Browser lit `download.request().uri()` → `source_url`.
2. Browser lit `download.suggested_filename()` → `declared_name`.
3. Browser lit `download.response().mime_type()` quand dispo (peut être vide).
4. Construit `download_policy::Request { suggested_filename, mimetype,
   initiator_url, final_url, content_length, mode }` et appelle
   `download_policy::analyze(&req)` → `Report` pre-flight.
5. Si `report.verdict == Block` :
   - `download.cancel()` immédiat.
   - log `sec_log::emit(Block, "download blocked pre-flight: <reasons>")`.
   - **Ne pas** créer d'entrée dans le store (rien à afficher).
   - return.
6. Sinon, calcule le **temp path** via `nyx-core::downloads::staging::build_temp_path(temp_root, now_unix, &report.normalized_filename)`.
   - 16 octets random hex + nom sanitizé + extension préservée
     (l'extension reste car le sniff post-flight la cross-check).
7. Crée l'entrée `DownloadStore::add(report.normalized_filename,
   temp_path.to_string(), report.kind, report.final_origin, now)`.
8. Stocke un mapping `DownloadId ↔ webkit Download` dans
   `Rc<RefCell<HashMap<DownloadId, Download>>>` local au bridge (pour
   pouvoir cancel depuis le store via UI plus tard).
9. Connecte `received-data`, `finished`, `failed` sur ce `Download`.

### Phase B — Pendant le DL (`received-data`)

Throttlé : update toutes les ~200 ms max (debounce simple via
`Cell<Instant>`). À chaque tick :
- `store.update_progress(id, received, total)`.
- Si popover ouvert, le refresh re-rendra à la prochaine ouverture / clic
  (pas de push proactif — KISS, le store est polled à l'ouverture).

### Phase C — `finished`

1. Browser lit le temp_path complet.
2. Re-sniff : `let head = read_first_8k(temp_path)?;
   let sniffed = magic_bytes::sniff(&head); let live_kind =
   magic_bytes::category(sniffed);`.
3. SHA-256 du fichier complet (streaming via `sha2::Sha256`,
   chunks de 64 KiB).
4. Re-évalue le verdict :
   - extension cohérente avec le sniff ?
     `magic_bytes::matches_extension(sniffed, &ext)` → si `false` et
     `sniffed != Unknown`, on traite comme **MIME mismatch sévère**.
   - Le mode est relu depuis settings (peut avoir changé pendant le DL).
5. Décide le **Verdict post-flight** :
   - Block (delete temp + mark_failed + return).
   - Allow (move temp→final + write .nyxmeta + mark_completed).
   - Ask / AskDanger → dialog confirmation UI.
6. **Confirmation UI** (réutilise pattern de `actions::confirm_then_open`,
   2 niveaux : Ask = bouton standard, AskDanger = bouton style
   `destructive-action` + message warning fort).
   - OK : move + meta + mark_completed.
   - Cancel : delete temp + mark_cancelled.

### Phase D — `failed`

`download.failed()` callback :
- delete temp file (best-effort, ignore Result).
- `store.mark_failed(id, redacted_reason, now)`.

---

## 4. Temp dir et naming

Resolved at start :
```
temp_root = XDG_CACHE_HOME/nyx/downloads/   (Linux)
          = ~/Library/Caches/nyx/downloads/ (macOS)
          = %LOCALAPPDATA%\nyx\downloads\   (Windows)
```

Création best-effort au boot via `std::fs::create_dir_all`. Si échec
→ fallback `std::env::temp_dir().join("nyx-downloads")`.

Naming :
```
<32-hex-random>_<sanitized_filename>
```
- **32 hex = 128 bits** d'entropie (décision review user — confortable
  contre toute collision même sur 10⁹ DL par utilisateur).
- Random via `getrandom::getrandom()` (CSPRNG OS-fourni).
- `sanitized_filename` est le `normalized_filename` du `Report`
  pre-flight (no NUL, no `/`, no bidi, no Windows reserved).
- Pas de path traversal possible (le nom n'a pas de séparateur).

Permissions : pas de `chmod`, pas de `set_permissions`. Le fichier
hérite des perms du dossier (umask user). Sur Linux/macOS, le dossier
parent est créé avec mode 0700 si possible (`set_permissions` sur le
dossier — pas sur le fichier).

---

## 5. `.nyxmeta` sidecar — quarantine metadata (key=value strict)

**Format : key=value, une paire par ligne, pas de JSON.**

Décision suite à review user : éviter le JSON manuel (échappement
piégeux : `"`, `\`, newlines, bidi, controls). `serde_json` ajoute une
dépendance non-justifiée pour 10 champs scalaires figés. Un format
key=value strict avec règles de validation simples est plus défensif
que les deux.

### Format
```
schema=1
sha256=<64 chars 0-9a-f>
size_bytes=<u64>
sniffed=<Detected variant name>
declared_ext=<sanitized, lowercase>
declared_mime=<sanitized, lowercase>
source_host=<host only, no scheme/path/query>
final_host=<host only, après redirects>
started_at_unix=<i64>
finished_at_unix=<i64>
verdict=<Allow|Ask|AskDanger|Block>
reasons=<comma-separated Reason variants>
```

### Règles de validation (à l'écriture ET à la lecture)
- Une paire par ligne, séparateur `=` (premier `=` rencontré).
- Clés : `[a-z_]+` uniquement. Toute autre clé → ligne rejetée à
  l'écriture, ignorée silencieusement à la lecture.
- Valeurs : UTF-8 valide, AUCUN `\n` `\r` `\0`, pas de control char
  (`< 0x20`) sauf espace. Longueur ≤ 512 octets par valeur.
- Lignes vides et lignes commençant par `#` → ignorées (futur commentaire).
- Champs inconnus à la lecture → ignorés (forward compat).
- Champs requis manquants à la lecture → `parse` retourne `None`.
- Doublons : la dernière valeur gagne (forward compat sur format évolutif).

### Champs jamais inclus (defense-in-depth)
- ❌ URL complète avec path / query / fragment
- ❌ Headers HTTP
- ❌ Cookies
- ❌ Token / auth
- ❌ Full path absolu (juste le basename via le nom du sibling)
- ❌ User-Agent
- ❌ Referer

### Ordre d'écriture (séquence atomique-friendly)
1. Le payload est dans temp_path après `finished`.
2. Écriture du `.nyxmeta` à côté de temp_path (`<temp>.nyxmeta`).
3. `rename(temp_path → final_path)` (atomique POSIX si même FS).
4. `rename(<temp>.nyxmeta → <final>.nyxmeta)` (idem).

Si crash entre 3 et 4 : payload présent, meta absent → on affiche
« non-quarantined » dans la shelf (le hash et la décision sont
re-calculables sur lecture).

### Implémentation
`nyx-core/src/downloads/quarantine.rs` (pur) :
- `pub struct QuarantineMeta { …champs typés… }`
- `pub fn serialize(&self) -> String` (~30 lignes, validation + write).
- `pub fn parse(&str) -> Option<QuarantineMeta>` (~50 lignes,
  defensive, ignore unknown).
- Tests : round-trip, hostile inputs (NUL byte, newline, control,
  équal multiples, key invalide, valeur longue, doublons, unknown
  fields, version future).

---

## 6. Rollback / crash safety

### Au démarrage (boot scan)

`crates/browser/src/web/downloads/cleanup.rs` :
- Au boot du `WebContext`, scan `temp_root` avec ces règles strictes :
  - **canonicalize le path** (suit les `..` et liens dans le PARENT,
    pas l'entrée). Si `canonical(entry).starts_with(canonical(temp_root))`
    est faux → skip (paranoïa anti-symlink).
  - Vérifie `metadata().file_type().is_symlink()` → skip si oui.
  - fichiers > 24h (`modified()` vs `now`) → delete.
  - **À tout instant le scan ne quitte pas temp_root** (pas de
    récursion ; un seul `read_dir`).
  - Erreurs `remove_file` ignorées silencieusement (best-effort).
- Failed download : delete temp IMMÉDIATEMENT dans le callback `failed`
  (pas attendre le boot — limite la fenêtre d'orphelin).

### Au cancel UI (à venir, hors de cette Tâche)

API exposée pour plus tard : `bridge.cancel(id)` → `download.cancel()` +
delete temp + mark_cancelled. Pas implémenté maintenant (KISS : on
ajoutera quand l'UI aura le bouton "cancel in-progress").

### Si WebKit crash mid-download

L'objet `Download` est lost, le temp file orphelin. Boot scan le
nettoie au prochain démarrage (24h cap). Acceptable.

---

## 7. Sécurité — invariants garantis

Liste exhaustive issue de la review user, à respecter par construction
ET vérifier par tests/inspection lors de chaque PR touchant le bridge :

| #  | Invariant                                                      | Garantie / mécanisme                              |
|----|----------------------------------------------------------------|---------------------------------------------------|
| 1  | Temp dir créé en mode 0700 si possible                         | `set_permissions` sur le dossier au boot          |
| 2  | Payload jamais ouvert automatiquement                          | aucun `platform::open_path` post-DL               |
| 3  | Destination finale **uniquement** après policy verdict         | move temp→final fait dans la branche Allow/Yes    |
| 4  | Aucun `chmod +x` sur le payload                                | jamais d'appel `set_permissions` sur les fichiers |
| 5  | Nom final toujours sanitized                                   | `download_policy::Report::normalized_filename`    |
| 6  | Filename serveur jamais trusté                                 | `Content-Disposition` passe par sanitize          |
| 7  | MIME serveur jamais trusté seul                                | `magic_bytes::sniff` post-flight prioritaire      |
| 8  | Magic bytes priorisent sur l'extension                         | `matches_extension` croise les deux               |
| 9  | Redirection finale ré-analysée                                 | `download_policy::Request::final_url` post-DL     |
| 10 | Dangerous origin bloque même type safe                         | déjà dans `download_policy::synthesize_verdict`   |
| 11 | `.nyxmeta` jamais URL complète avec query                      | meta n'expose que `source_host` / `final_host`    |
| 12 | Boot scan ne suit jamais les symlinks                          | canonicalize + `is_symlink()` check               |
| 13 | `Verdict::Block` toujours respecté                             | cancel + delete + mark_failed sans détour         |
| 14 | Pas de tokens / cookies dans les logs                          | `sec_log::redact_url` + host-only                 |
| 15 | Pas de race write↔execute                                      | aucun exec dans cette Tâche                       |
| 16 | `recheck_on_run` toujours respecté côté shelf                  | pipeline DL indépendante (post-flight ≠ post-run) |
| 17 | Failed delete temp **immédiat** (pas seulement boot)           | callback `failed` dans `bridge.rs`                |
| 18 | Random temp suffix CSPRNG 128 bits                             | `getrandom::getrandom([0u8; 16])`                 |

---

## 8. Tests

### nyx-core (pur, en CI/clippy/test)

- `quarantine` : serialize/parse round-trip, hostile JSON (NUL, BOM,
  fields manquants, version inconnue), stress 1k.
- `staging` :
  - random suffix bien formé (hex, 16 chars).
  - sanity-check du final path.
  - panic-test sur entrées tordues.
  - stress 10k.

### Pages hostiles (tests/pages/)

- `download_exe_disguised.html` : `<a download="invoice.pdf" href="...exe">`.
- `download_double_ext.html` : `facture.pdf.exe`.
- `download_bidi.html` : nom avec U+202E.
- `download_size_huge.html` : `Content-Length: 10000000000`.
- `download_mime_mismatch.html` : ext .png, MIME application/x-msdownload.

Ces pages SONT du contenu statique. Le test = lancer Nyx dessus
manuellement (vu qu'on ne peut pas headless WebKit en CI ici), vérifier
visuellement que le bridge réagit correctement (Block / AskDanger /
sanitized name dans la shelf).

### Pas de mock-WebView CI

Le bridge dépend de signaux WebKit qu'on ne peut pas mocker proprement
en pur Rust. Les tests d'intégration restent **manuels** sur les pages
hostiles. Les modules `nyx-core` ont 100% des tests automatisés.

### TODO future (noté pour ne pas perdre)

```
TODO[future]: xvfb + gtk smoke test
  - Lance Nyx headless dans xvfb-run
  - Charge tests/pages/download_*.html via une route locale
  - Vérifie que la shelf affiche le verdict attendu via inspecteur
  - Probablement après une refonte du runtime (webkit2gtk 6 +
    headless mode amélioré)
```

Pas pour cette Tâche. Mais le placeholder est là pour qu'on n'oublie
pas d'y revenir.

---

## 9. Plan de fichiers (≤ 250 lignes / fichier)

### nyx-core (pur, 3 fichiers + tests)

```
crates/nyx-core/src/downloads/
  quarantine.rs   (~180 l.)  JSON minimal sans serde, schema=1
  staging.rs      (~120 l.)  random suffix, temp path build
  quarantine/tests.rs (~150 l.)  round-trip + hostile
  staging/tests.rs    (~120 l.)  hostile + stress 10k
```

Cargo.toml :
- ajouter `sha2 = "0.10"` (RustCrypto, pure Rust, audité — pas de C deps).
- ajouter `getrandom = "0.2"` pour le random suffix. (déjà présente via
  `idna`'s transitive ? à vérifier — sinon ajout direct).

### browser (GTK/WebKit, 5 fichiers)

```
crates/browser/src/web/downloads/
  mod.rs        (~50 l.)   install(context, store, settings) → wire signals
  bridge.rs     (~200 l.)  download_started → decide_destination → finished/failed
  io.rs         (~150 l.)  first8k read, sha256 stream, move atomic, meta write
  cleanup.rs    (~80 l.)   boot scan + 24h purge
  confirm.rs    (~120 l.)  dialog Ask / AskDanger (réutilise widgets/style)
```

### State

```
crates/browser/src/state/downloads_temp.rs  (~40 l.)
   pub fn temp_root() -> PathBuf
```

### Wire main

```rust
// main.rs (~3 lignes ajoutées)
let temp_root = state::downloads_temp::temp_root();
web::downloads::install(&web_context, dls.clone(), prefs.clone(), temp_root);
```

---

## 10. Branches proposées

Découpage en deux branches `--no-ff` :

1. `feat/download-bridge-core` — nyx-core uniquement (quarantine +
   staging + sha2 dep). Tests massifs. Mergeable seul.
2. `feat/download-bridge-webkit` — browser/web/downloads + wire +
   pages hostiles. Dépend de (1).

Découpage justifié : (1) est mergeable et testable en isolation ; (2)
ne peut être verified que via `cargo run` sur le user setup.

---

## 11. Risques résiduels (transparence)

- Dépendances ajoutées : `sha2`, possiblement `getrandom`. Audit OK
  (RustCrypto + std-rust). Justification : impossible de calculer un
  SHA-256 propre sans crypto crate. `getrandom` est de facto le std
  pour le random sécurisé en Rust.
- Pages hostiles non couvertes en CI : compromis pragmatique. Les
  modules purs ont 100% des tests, le bridge a 0% (manuel uniquement).
- `received-data` throttling à 200 ms : si un site fait 1000 chunks/s,
  l'UI ne le voit pas. C'est intentionnel — KISS, pas de spam.
- Boot scan `temp_root` ne distingue pas un fichier Nyx d'un autre
  fichier dans le même dossier. Mitigation : `temp_root` est dédié à
  Nyx (cache/nyx/downloads). Si l'utilisateur y met autre chose à la
  main, on delete. Acceptable.

---

## 12. Ce qui n'est PAS dans la Tâche 5

- Persistance du `DownloadStore` (rebooter Nyx perd la shelf history).
  → futur ticket.
- Bouton « Annuler » sur les rows in-progress dans la shelf.
  → futur ticket.
- Notification système desktop quand DL fini.
  → futur ticket (libnotify ou portal).
- Resume DL interrompu.
  → futur ticket, complexe (HTTP Range, state sur disque).

---

## 13. Décisions finales (après review user)

| # | Décision                                                | Statut                                                                  |
|---|---------------------------------------------------------|-------------------------------------------------------------------------|
| 1 | `sha2` + `getrandom` dans `nyx-core/Cargo.toml`         | ✅ validé                                                               |
| 2 | Format `.nyxmeta`                                       | 🔄 **key=value strict** (Option B review) — pas de JSON manuel ni serde |
| 3 | 2 branches successives `-core` puis `-webkit`           | ✅ validé                                                               |
| 4 | Tests WebKit = pages hostiles manuelles                 | ✅ validé pour MVP + TODO future xvfb (§8)                              |
| 5 | `.nyxmeta` sidecar (pas DB)                             | ✅ validé                                                               |
| 6 | Boot scan 24h                                           | ✅ validé + no symlink + canonical path + failed delete immédiat        |

**Décisions additionnelles issues de la review** :
- Random suffix : **32 hex (128 bits)** au lieu de 16 hex.
- `.nyxmeta` : `source_host` / `final_host` uniquement, jamais d'URL.
- Invariants sécurité : §7 étendu de 11 à 18 règles explicites.
- Temp dir parent : mode 0700 si possible (Unix-only).

Prochaine action : `feat/download-bridge-core` (nyx-core uniquement).
