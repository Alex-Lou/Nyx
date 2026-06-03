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
<16-hex-random>_<sanitized_filename>
```
- 16 octets random via `getrandom` (collision quasi-impossible, pas
  prévisible).
- `sanitized_filename` est déjà le `normalized_filename` du `Report`
  pre-flight (no NUL, no `/`, no bidi, no Windows reserved).
- Pas de path traversal possible (le nom n'a pas de séparateur).

Permissions : pas de `chmod`, pas de `set_permissions`. Le fichier
hérite des perms du dossier (umask user).

---

## 5. `.nyxmeta` sidecar — quarantine metadata

Format : JSON, écrit à côté du fichier final (final_path + ".nyxmeta").

Contenu :
```json
{
  "schema": 1,
  "sha256": "abcdef…64chars",
  "size_bytes": 12345,
  "sniffed": "Png",
  "declared_ext": "png",
  "declared_mime": "image/png",
  "source_origin": "https://example.com",
  "started_at_unix": 1700000000,
  "finished_at_unix": 1700000050,
  "verdict": "Allow",
  "reasons": ["SafeType"]
}
```

Pas de path absolu dans le meta (juste basename via le nom du fichier
sibling). Pas de query string. Pas de cookie. Pas de token.

Écrit AVANT le rename temp→final, dans le temp_root, puis déplacé en
même temps que le fichier (un seul `rename` atomique côté FS pour les
deux ? Non — deux renames : data d'abord, meta ensuite. Si crash entre
les deux, meta sera absent au démarrage suivant → on n'utilise pas le
fichier pour autant, on signale juste "non-quarantined" dans la shelf).

`nyx-core/src/downloads/quarantine.rs` (pur) :
- `QuarantineMeta { …champs… }`
- `serialize(&self) -> String` (sérialise en JSON manuellement, sans
  serde — la struct est petite et figée).
- `parse(&str) -> Option<QuarantineMeta>` (parse minimal, robuste aux
  champs inconnus / version future).
- Tests : round-trip, hostile inputs.

---

## 6. Rollback / crash safety

### Au démarrage (boot scan)

`crates/browser/src/web/downloads/cleanup.rs` :
- Au boot du `WebContext`, scan `temp_root` :
  - fichiers plus vieux que 24h → delete (best-effort).
  - fichiers sans entrée store correspondante (le store ne persiste pas
    encore — donc TOUS au boot) → delete.
- Pas de scan récursif. Pas de symlink follow.

### Au cancel UI (à venir, hors de cette Tâche)

API exposée pour plus tard : `bridge.cancel(id)` → `download.cancel()` +
delete temp + mark_cancelled. Pas implémenté maintenant (KISS : on
ajoutera quand l'UI aura le bouton "cancel in-progress").

### Si WebKit crash mid-download

L'objet `Download` est lost, le temp file orphelin. Boot scan le
nettoie au prochain démarrage (24h cap). Acceptable.

---

## 7. Sécurité — invariants garantis

| Règle                                                  | Garantie                              |
|--------------------------------------------------------|---------------------------------------|
| Aucun `chmod +x`                                       | jamais d'appel `set_permissions`      |
| Aucun auto-open                                        | aucun `platform::open_path` post-DL   |
| `Verdict::Block` toujours respecté                     | cancel + delete + mark_failed         |
| Re-sniff fait avant move final                         | post-flight obligatoire               |
| MIME mismatch détecté                                  | déjà dans `download_policy::analyze`  |
| Pas de chemin web non-sanitizé en CLI                  | filenames passent `download_policy`   |
| Pas de full path dans `.nyxmeta`                       | basename uniquement                   |
| Pas de tokens / cookies dans les logs                  | `sec_log::redact_url`                 |
| Pas de symlink follow au scan boot                     | `read_dir + remove_file` direct       |
| Pas de race write↔execute                              | jamais d'exec dans cette Tâche        |
| `recheck_on_run` toujours respecté côté shelf          | la pipeline DL est indépendante       |

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

## 13. Question pour validation

Je peux procéder à la Tâche 5 dans cette forme si tu confirmes :

1. ✅ Ajout des crates `sha2` et `getrandom` à `nyx-core/Cargo.toml`.
2. ✅ Format JSON inline (pas de `serde_json` dep — la struct est figée
   et la sérialisation manuelle reste < 50 lignes).
3. ✅ Découpage en 2 branches successives (`-core` puis `-webkit`).
4. ✅ Tests d'intégration WebKit = pages hostiles + vérification manuelle.
   Pas de mock WebView CI.
5. ✅ `.nyxmeta` sidecar JSON (pas de DB — SQLCipher reste Sprint 2).
6. ✅ Boot scan du temp dir avec cap 24h.

Si tu valides ces 6 points, je commence par `feat/download-bridge-core`.
Si l'un te déplaît, dis-le maintenant — coût du change = zéro tant que
rien n'est codé.
