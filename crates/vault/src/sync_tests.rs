//! Tests du schéma v3 et des règles de fusion (sans réseau).

use super::*;
use rusqlite::Connection;
use std::path::PathBuf;

struct Tmp(PathBuf);

impl Tmp {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("nyx_vault_sync_{}_{}.db", std::process::id(), name));
        let _ = std::fs::remove_file(&p);
        Self(p)
    }
    fn open(&self) -> Vault {
        Vault::open(&self.0, "pw").expect("open")
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", self.0.display()));
        }
    }
}

/// Envoie à `to` tout ce que `from` a de nouveau pour lui (comme une session).
fn transfer(from: &Vault, to: &Vault) -> ApplyStats {
    let mut total = ApplyStats::default();
    for t in Table::ALL {
        loop {
            let w = to.sync_watermark(from.device_id(), t).unwrap();
            let batch = from.sync_changes(t, w, 200).unwrap();
            let Some(upto) = batch.last().map(|c| c.local_hlc) else { break };
            let recs: Vec<Record> = batch.into_iter().map(|c| c.record).collect();
            let s = to.sync_apply(&recs, Some((from.device_id(), t, upto))).unwrap();
            total.applied += s.applied;
            total.skipped += s.skipped;
            total.rejected += s.rejected;
        }
    }
    total
}

fn all(v: &Vault, t: Table) -> Vec<Record> {
    let mut r: Vec<Record> = v.sync_changes(t, 0, usize::MAX).unwrap().into_iter().map(|c| c.record).collect();
    r.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    r
}

fn assert_converged(a: &Vault, b: &Vault) {
    for t in Table::ALL {
        assert_eq!(all(a, t), all(b, t), "divergence sur {t:?}");
    }
}

#[test]
fn migration_v2_vers_v3_remplit_les_lignes_existantes() -> Result<()> {
    let tmp = Tmp::new("mig3");
    {
        let conn = Connection::open(&tmp.0)?;
        conn.pragma_update(None, "key", "pw")?;
        conn.execute_batch(
            "CREATE TABLE bookmarks (id INTEGER PRIMARY KEY AUTOINCREMENT, url TEXT NOT NULL, title TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '[]', created_at TEXT NOT NULL, folder TEXT NOT NULL DEFAULT '');
             CREATE TABLE passwords (id INTEGER PRIMARY KEY AUTOINCREMENT, domain TEXT NOT NULL,
                username TEXT NOT NULL, password TEXT NOT NULL, created_at TEXT NOT NULL);
             CREATE TABLE history (id INTEGER PRIMARY KEY AUTOINCREMENT, url TEXT NOT NULL,
                title TEXT NOT NULL, visited_at TEXT NOT NULL);
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO bookmarks (url, title, created_at) VALUES ('https://a.org', 'A', '2020-01-01T00:00:00+00:00');
             INSERT INTO bookmarks (url, title, created_at) VALUES ('https://b.org', 'B', '2020-01-01T00:00:00+00:00');
             INSERT INTO passwords (domain, username, password, created_at) VALUES ('a.org', 'u', 'p', '2020-01-01T00:00:00+00:00');
             INSERT INTO history (url, title, visited_at) VALUES ('https://h.org', 'H', '2020-01-01T00:00:00+00:00');
             INSERT INTO history (url, title, visited_at) VALUES ('https://h.org', 'H', '2020-01-01T00:00:00+00:00');
             INSERT INTO settings (key, value) VALUES ('app_settings', 'engine=brave');
             PRAGMA user_version = 2;",
        )?;
    }
    let v = tmp.open();
    assert_eq!(v.device_id().len(), 32);
    let bms = all(&v, Table::Bookmarks);
    assert_eq!(bms.len(), 2);
    assert_ne!(bms[0].uuid, bms[1].uuid);
    for t in Table::ALL {
        for r in all(&v, t) {
            assert!(r.updated_at > 0 && !r.deleted);
            assert_eq!(r.origin_device, v.device_id());
        }
    }
    let h = all(&v, Table::History);
    assert_eq!(h.len(), 1, "doublon exact fusionné");
    assert_eq!(h[0].uuid, sync::history_uuid("https://h.org", "2020-01-01T00:00:00+00:00"));
    assert_eq!(all(&v, Table::Settings)[0].uuid, sync::setting_uuid("app_settings"));
    // Les API existantes fonctionnent toujours.
    assert_eq!(v.passwords_for("a.org")?[0].password, "p");
    assert_eq!(v.setting("app_settings")?.as_deref(), Some("engine=brave"));
    // Réouverture : pas de re-migration, l'identité de l'appareil est stable.
    let id = v.device_id().to_string();
    drop(v);
    let v = tmp.open();
    assert_eq!(v.device_id(), id);
    assert_eq!(all(&v, Table::Bookmarks), bms);
    Ok(())
}

#[test]
fn chaque_ecriture_horodate_et_supprimer_cree_une_pierre_tombale() -> Result<()> {
    let tmp = Tmp::new("stamps");
    let v = tmp.open();
    let id = v.add_password(&Password::new("a.org", "u", "p1"))?;
    let r0 = all(&v, Table::Passwords).remove(0);
    v.update_password(id, "p2")?;
    let r1 = all(&v, Table::Passwords).remove(0);
    assert_eq!(r0.uuid, r1.uuid);
    assert!(r1.updated_at > r0.updated_at);
    v.delete_password(id)?;
    assert!(v.passwords_for("a.org")?.is_empty());
    assert!(v.list_password_domains()?.is_empty());
    let r2 = all(&v, Table::Passwords).remove(0);
    assert!(r2.deleted && r2.updated_at > r1.updated_at);
    assert_eq!(r2.data, r2.data.blanked(), "secret effacé de la pierre tombale");

    let b = v.add_bookmark(&Bookmark::new("https://a.org", "A"))?;
    v.delete_bookmark(b)?;
    assert!(v.list_bookmarks()?.is_empty());
    assert!(v.search_bookmarks("a.org")?.is_empty());
    assert!(all(&v, Table::Bookmarks)[0].deleted);

    v.push_history("https://h.org", "")?;
    let h0 = all(&v, Table::History).remove(0);
    assert!(v.retitle_history("https://h.org", "H")?);
    let h1 = all(&v, Table::History).remove(0);
    assert!(h1.updated_at > h0.updated_at);
    v.clear_history()?;
    assert!(v.recent_history(10)?.is_empty());
    assert!(all(&v, Table::History).iter().all(|r| r.deleted && r.data == r.data.blanked()));
    assert!(!v.retitle_history("https://h.org", "X")?);

    v.set_setting("k", "v")?;
    let s0 = all(&v, Table::Settings).remove(0);
    v.set_setting("k", "v")?;
    assert_eq!(all(&v, Table::Settings).remove(0), s0, "même valeur → pas de changement");
    v.set_setting("local.window", "1")?;
    assert_eq!(v.setting("local.window")?.as_deref(), Some("1"));
    assert_eq!(all(&v, Table::Settings).len(), 1, "réglage local jamais exporté");

    // Purge des pierres tombales (TTL nul → toutes, strictement antérieures).
    std::thread::sleep(std::time::Duration::from_millis(3));
    assert!(v.purge_tombstones(0)? >= 3);
    assert!(all(&v, Table::Passwords).is_empty());
    Ok(())
}

#[test]
fn replace_bookmarks_garde_l_identite() -> Result<()> {
    let tmp = Tmp::new("replace_ident");
    let v = tmp.open();
    v.replace_bookmarks(&[Bookmark::new("https://a.org", "A"), Bookmark::new("https://b.org", "B")])?;
    let before = all(&v, Table::Bookmarks);
    v.replace_bookmarks(&[Bookmark::new("https://a.org", "A"), Bookmark::new("https://b.org", "B")])?;
    assert_eq!(all(&v, Table::Bookmarks), before, "rien n'a changé → rien à synchroniser");
    v.replace_bookmarks(&[Bookmark::new("https://a.org", "A2")])?;
    let after = all(&v, Table::Bookmarks);
    assert_eq!(after.len(), 2);
    let a = after.iter().find(|r| matches!(&r.data, RecordData::Bookmark { title, .. } if title == "A2")).unwrap();
    assert!(before.iter().any(|r| r.uuid == a.uuid), "uuid conservé");
    assert_eq!(after.iter().filter(|r| r.deleted).count(), 1);
    Ok(())
}

#[test]
fn modifications_concurrentes_le_dernier_ecrivain_gagne() -> Result<()> {
    let (ta, tb) = (Tmp::new("lww_a"), Tmp::new("lww_b"));
    let (a, b) = (ta.open(), tb.open());
    let id_a = a.add_password(&Password::new("x.org", "u", "v0"))?;
    transfer(&a, &b);
    let id_b = b.passwords_for("x.org")?[0].id.unwrap();
    // Édition concurrente : B écrit après A → B gagne partout.
    a.update_password(id_a, "from-a")?;
    std::thread::sleep(std::time::Duration::from_millis(2));
    b.update_password(id_b, "from-b")?;
    transfer(&a, &b);
    transfer(&b, &a);
    assert_eq!(a.passwords_for("x.org")?[0].password, "from-b");
    assert_eq!(b.passwords_for("x.org")?[0].password, "from-b");
    assert_converged(&a, &b);
    // Rejouer : idempotent.
    let again = transfer(&a, &b);
    assert_eq!(again.applied, 0);
    Ok(())
}

#[test]
fn egalite_de_hlc_departagee_par_l_appareil() -> Result<()> {
    let (ta, tb) = (Tmp::new("tie_a"), Tmp::new("tie_b"));
    let (a, b) = (ta.open(), tb.open());
    let at = hlc::from_millis(hlc::now_millis());
    let rec = |dev: &str, v: &str| Record {
        uuid: sync::setting_uuid("k"),
        updated_at: at,
        deleted: false,
        origin_device: dev.into(),
        data: RecordData::Setting { key: "k".into(), value: v.into() },
    };
    let (lo, hi) = (rec("aaaa", "lo"), rec("ffff", "hi"));
    a.sync_apply(&[lo.clone(), hi.clone()], None)?;
    b.sync_apply(&[hi, lo], None)?;
    assert_eq!(a.setting("k")?.as_deref(), Some("hi"));
    assert_eq!(b.setting("k")?.as_deref(), Some("hi"));
    Ok(())
}

#[test]
fn suppression_contre_edition() -> Result<()> {
    let (ta, tb) = (Tmp::new("del_a"), Tmp::new("del_b"));
    let (a, b) = (ta.open(), tb.open());
    let ia = a.add_bookmark(&Bookmark::new("https://x.org", "X"))?;
    a.add_password(&Password::new("y.org", "u", "p"))?;
    transfer(&a, &b);
    let ib = b.list_bookmarks()?[0].id.unwrap();
    let pb = b.passwords_for("y.org")?[0].id.unwrap();

    // 1) Édition puis suppression plus récente : la pierre tombale gagne.
    b.replace_bookmarks(&[Bookmark::new("https://x.org", "X édité")])?;
    std::thread::sleep(std::time::Duration::from_millis(2));
    a.delete_bookmark(ia)?;
    transfer(&a, &b);
    transfer(&b, &a);
    assert!(a.list_bookmarks()?.is_empty() && b.list_bookmarks()?.is_empty());
    let _ = ib;

    // 2) Suppression puis édition plus récente : l'édition gagne (ressuscite).
    let pa = a.passwords_for("y.org")?[0].id.unwrap();
    a.delete_password(pa)?;
    std::thread::sleep(std::time::Duration::from_millis(2));
    b.update_password(pb, "nouveau")?;
    transfer(&a, &b);
    transfer(&b, &a);
    assert_eq!(a.passwords_for("y.org")?[0].password, "nouveau");
    assert_converged(&a, &b);

    // 3) Une mise à jour plus ANCIENNE arrivant après la pierre tombale est ignorée.
    let stale = all(&a, Table::Bookmarks).remove(0);
    assert!(stale.deleted);
    let mut old = stale.clone();
    old.deleted = false;
    old.updated_at -= 1;
    old.data = RecordData::Bookmark {
        url: "https://x.org".into(),
        title: "zombie".into(),
        folder: String::new(),
        tags: vec![],
        created_at: String::new(),
    };
    let s = a.sync_apply(&[old], None)?;
    assert_eq!(s.skipped, 1);
    assert!(a.list_bookmarks()?.is_empty());
    Ok(())
}

#[test]
fn historique_fusionne_comme_un_ensemble() -> Result<()> {
    let (ta, tb) = (Tmp::new("hist_a"), Tmp::new("hist_b"));
    let (a, b) = (ta.open(), tb.open());
    let e = |u: &str, t: i64| (u.to_string(), String::new(), t);
    // Même import Chrome des deux côtés + visites propres.
    let common = [e("https://c1.org", 1_600_000_000), e("https://c2.org", 1_600_000_100)];
    a.import_history(&common)?;
    b.import_history(&common)?;
    a.push_history("https://only-a.org", "A")?;
    b.push_history("https://only-b.org", "B")?;
    transfer(&a, &b);
    transfer(&b, &a);
    assert_eq!(a.recent_history(100)?.len(), 4);
    assert_eq!(b.recent_history(100)?.len(), 4);
    assert_converged(&a, &b);
    // Effacement propagé.
    b.clear_history()?;
    transfer(&b, &a);
    assert!(a.recent_history(100)?.is_empty());
    assert_converged(&a, &b);
    Ok(())
}

#[test]
fn historique_reste_borne_apres_fusion() -> Result<()> {
    let (ta, tb) = (Tmp::new("hbound_a"), Tmp::new("hbound_b"));
    let (a, b) = (ta.open(), tb.open());
    let cap = history::MAX_ENTRIES;
    let mk = |p: &str, base: i64| -> Vec<(String, String, i64)> {
        (0..cap).map(|i| (format!("https://{p}.org/{i}"), String::new(), base + 2 * i)).collect()
    };
    a.import_history(&mk("a", 1_600_000_000))?;
    b.import_history(&mk("b", 1_600_000_001))?;
    transfer(&a, &b);
    transfer(&b, &a);
    let ha = a.recent_history(2 * cap as usize)?;
    let hb = b.recent_history(2 * cap as usize)?;
    assert_eq!(ha.len(), cap as usize);
    let urls = |h: &[HistoryEntry]| h.iter().map(|e| e.url.clone()).collect::<Vec<_>>();
    assert_eq!(urls(&ha), urls(&hb), "même ensemble des plus récentes");
    Ok(())
}

#[test]
fn lot_rejoue_et_filigrane_persistant() -> Result<()> {
    let (ta, tb) = (Tmp::new("wm_a"), Tmp::new("wm_b"));
    let (a, b) = (ta.open(), tb.open());
    for i in 0..20 {
        a.add_bookmark(&Bookmark::new(format!("https://{i}.org"), "t"))?;
    }
    let batch = a.sync_changes(Table::Bookmarks, 0, 5)?;
    assert_eq!(batch.len(), 5);
    let upto = batch.last().unwrap().local_hlc;
    let recs: Vec<Record> = batch.into_iter().map(|c| c.record).collect();
    assert_eq!(b.sync_apply(&recs, Some((a.device_id(), Table::Bookmarks, upto)))?.applied, 5);
    assert_eq!(b.sync_apply(&recs, Some((a.device_id(), Table::Bookmarks, upto)))?.skipped, 5);
    assert_eq!(b.sync_watermark(a.device_id(), Table::Bookmarks)?, upto);
    // Reprise : seulement le reste.
    let rest = transfer(&a, &b);
    assert_eq!(rest.applied, 15);
    assert_eq!(b.list_bookmarks()?.len(), 20);
    b.sync_forget_peer(a.device_id())?;
    assert_eq!(b.sync_watermark(a.device_id(), Table::Bookmarks)?, 0);
    Ok(())
}

#[test]
fn changements_d_egale_hlc_jamais_coupes() -> Result<()> {
    let tmp = Tmp::new("ties");
    let v = tmp.open();
    let at = hlc::from_millis(hlc::now_millis());
    let recs: Vec<Record> = (0..10)
        .map(|i| Record {
            uuid: format!("{i:032x}"),
            updated_at: at,
            deleted: false,
            origin_device: "peer".into(),
            data: RecordData::Bookmark {
                url: format!("https://{i}.org"),
                title: String::new(),
                folder: String::new(),
                tags: vec![],
                created_at: String::new(),
            },
        })
        .collect();
    v.sync_apply(&recs, None)?;
    // Force la même local_hlc sur toutes les lignes.
    v.conn.execute("UPDATE bookmarks SET local_hlc = 42", [])?;
    let batch = v.sync_changes(Table::Bookmarks, 0, 3)?;
    assert_eq!(batch.len(), 10, "le lot s'étend à toutes les lignes de même local_hlc");
    assert!(v.sync_changes(Table::Bookmarks, 42, 3)?.is_empty());
    Ok(())
}

#[test]
fn enregistrements_invalides_rejetes() -> Result<()> {
    let tmp = Tmp::new("reject");
    let v = tmp.open();
    let future = Record {
        uuid: sync::setting_uuid("k"),
        updated_at: hlc::from_millis(hlc::now_millis() + 2 * hlc::MAX_DRIFT_MS),
        deleted: false,
        origin_device: "p".into(),
        data: RecordData::Setting { key: "k".into(), value: "v".into() },
    };
    let s = v.sync_apply(&[future], None)?;
    assert_eq!(s.rejected, 1);
    assert_eq!(v.setting("k")?, None);
    Ok(())
}
