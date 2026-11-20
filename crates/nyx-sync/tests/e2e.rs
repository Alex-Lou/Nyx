//! Bout en bout sur 127.0.0.1 : deux (ou trois) coffres SQLCipher
//! temporaires, appairage réel (Noise XXpsk3), synchronisations réelles
//! (Noise KK), modifications concurrentes, convergence, attaques.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use nyx_sync::pairing::{self, Offer};
use nyx_sync::{session, Device, SyncEngine, SyncError, SyncReport, Table};
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use vault::{Bookmark, Password, Record, Vault};

const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

struct Tmp(PathBuf);

impl Tmp {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("nyx_sync_e2e_{}_{name}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        Self(p)
    }
    fn vault(&self) -> Vault {
        Vault::open(&self.0, "pw").expect("open vault")
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", self.0.display()));
        }
    }
}

fn engine(v: &Vault) -> SyncEngine<'_> {
    let mut e = SyncEngine::open(v).expect("engine");
    e.set_io_timeout(Duration::from_secs(5));
    e
}

fn sync_err(e: &anyhow::Error) -> Option<SyncError> {
    e.downcast_ref::<SyncError>().cloned()
}

/// Appaire `guest` au profil de `host` ; vérifie que les deux codes affichés
/// sont identiques. Retourne (appareil vu par l'hôte, hôte vu par l'invité).
fn pair(host: &Tmp, guest: &Tmp, guest_name: &str) -> (Device, Device) {
    let listener = TcpListener::bind((LOCAL, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (qr_tx, qr_rx) = mpsc::channel();
    let path = host.0.clone();
    let h = thread::spawn(move || {
        let v = Vault::open(&path, "pw").unwrap();
        let e = engine(&v);
        let mut offer = Offer::new(&e, vec![LOCAL], port).unwrap();
        qr_tx.send(offer.encode()).unwrap();
        let mut code = String::new();
        let dev = pairing::host(&e, &listener, &mut offer, |c, _| {
            code = c.to_string();
            true
        });
        // Usage unique : la même offre ne resert pas.
        let again = pairing::host(&e, &listener, &mut offer, |_, _| true).unwrap_err();
        assert_eq!(sync_err(&again), Some(SyncError::TokenUsed));
        (dev.unwrap(), code)
    });
    let qr = qr_rx.recv().unwrap();
    let v = guest.vault();
    let e = engine(&v);
    let mut guest_code = String::new();
    let host_dev = pairing::accept(&qr, &e, guest_name, |c, _| {
        guest_code = c.to_string();
        true
    })
    .expect("accept");
    let (guest_dev, host_code) = h.join().unwrap();
    assert_eq!(host_code.len(), 6);
    assert_eq!(host_code, guest_code, "codes de confirmation identiques");
    assert_eq!(guest_dev.device_id, v.device_id());
    assert_eq!(guest_dev.name, guest_name);
    (guest_dev, host_dev)
}

/// `client` se synchronise avec `server`, joint par IP « manuelle » : le
/// client essaie ses pairs un par un, le serveur boucle comme le ferait une
/// coquille (une connexion d'essai ratée n'arrête pas le service).
fn sync(server: &Tmp, client: &Tmp) -> (SyncReport, SyncReport) {
    let listener = TcpListener::bind((LOCAL, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let path = server.0.clone();
    let h = thread::spawn(move || {
        let v = Vault::open(&path, "pw").unwrap();
        let e = engine(&v);
        let mut last = None;
        for _ in 0..4 {
            match session::serve_one(&e, &listener) {
                Ok(r) => return Ok(r),
                Err(err) => last = Some(err),
            }
        }
        Err(last.unwrap())
    });
    let v = client.vault();
    let c = session::sync_with(&engine(&v), addr, None).expect("sync_with");
    let s = h.join().unwrap().expect("serve_one");
    (s, c)
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
fn appairage_puis_synchronisation_dans_les_deux_sens_et_convergence() {
    let (pc, phone) = (Tmp::new("conv_pc"), Tmp::new("conv_phone"));
    // Données préexistantes des deux côtés.
    {
        let v = pc.vault();
        v.add_password(&Password::new("bank.fr", "alex", "pc-secret")).unwrap();
        v.add_bookmark(&Bookmark::new("https://rust-lang.org", "Rust")).unwrap();
        v.add_bookmark(&Bookmark::new("https://old.org", "Old")).unwrap();
        v.push_history("https://pc.org", "PC").unwrap();
        v.set_setting("app_settings", "engine=brave").unwrap();
        v.set_setting("local.window", "pc").unwrap();
    }
    {
        let v = phone.vault();
        v.add_bookmark(&Bookmark::new("https://mobile.org", "Mobile")).unwrap();
        v.push_history("https://phone.org", "Phone").unwrap();
        v.set_setting("local.window", "phone").unwrap();
    }

    let (phone_dev, pc_dev) = pair(&pc, &phone, "Pixel");
    {
        // Les deux profils sont identiques (même secret → même étiquette).
        let (a, b) = (pc.vault(), phone.vault());
        let (ea, eb) = (engine(&a), engine(&b));
        assert_eq!(ea.discovery_tag().unwrap(), eb.discovery_tag().unwrap());
        assert_eq!(ea.profile().unwrap().secret(), eb.profile().unwrap().secret());
        assert!(ea.devices().unwrap().iter().any(|d| d.device_id == phone_dev.device_id));
        assert_eq!(pc_dev.device_id, a.device_id());
    }

    let (s, c) = sync(&pc, &phone);
    assert!(s.sent > 0 && c.sent > 0);
    assert_eq!(s.peer.device_id, phone_dev.device_id);
    assert_eq!(c.peer.device_id, pc_dev.device_id);
    {
        let (a, b) = (pc.vault(), phone.vault());
        assert_eq!(b.passwords_for("bank.fr").unwrap()[0].password, "pc-secret");
        assert_eq!(a.list_bookmarks().unwrap().len(), 3);
        assert_eq!(b.list_bookmarks().unwrap().len(), 3);
        assert_eq!(a.recent_history(10).unwrap().len(), 2);
        assert_eq!(b.setting("app_settings").unwrap().as_deref(), Some("engine=brave"));
        assert_eq!(b.setting("local.window").unwrap().as_deref(), Some("phone"), "réglage local intact");
        assert_eq!(a.setting("local.window").unwrap().as_deref(), Some("pc"));
        assert_converged(&a, &b);
    }

    // Modifications concurrentes hors ligne.
    {
        let a = pc.vault();
        let id = a.passwords_for("bank.fr").unwrap()[0].id.unwrap();
        a.update_password(id, "from-pc").unwrap();
        let old = a.list_bookmarks().unwrap().into_iter().find(|b| b.url == "https://old.org").unwrap();
        a.delete_bookmark(old.id.unwrap()).unwrap();
        a.set_setting("app_settings", "engine=ddg").unwrap();
    }
    thread::sleep(Duration::from_millis(5));
    {
        let b = phone.vault();
        let id = b.passwords_for("bank.fr").unwrap()[0].id.unwrap();
        b.update_password(id, "from-phone").unwrap(); // plus récent → gagne
        b.replace_bookmarks(&[
            Bookmark::new("https://rust-lang.org", "Rust (édité)"),
            Bookmark::new("https://mobile.org", "Mobile"),
            Bookmark::new("https://old.org", "Old"),
        ])
        .unwrap();
        b.push_history("https://phone2.org", "P2").unwrap();
    }

    // Dans l'autre sens cette fois : le PC initie.
    let (_, c2) = sync(&phone, &pc);
    assert!(c2.received.applied > 0);
    {
        let (a, b) = (pc.vault(), phone.vault());
        assert_converged(&a, &b);
        assert_eq!(a.passwords_for("bank.fr").unwrap()[0].password, "from-phone");
        assert_eq!(a.setting("app_settings").unwrap().as_deref(), Some("engine=ddg"));
        let bms = b.list_bookmarks().unwrap();
        assert!(bms.iter().any(|x| x.title == "Rust (édité)"));
        assert!(!bms.iter().any(|x| x.url == "https://old.org"), "suppression (plus récente que la vue du téléphone) gagne");
        assert_eq!(b.recent_history(10).unwrap().len(), 3);
    }

    // Idempotence : une nouvelle session n'applique rien.
    let (s3, c3) = sync(&pc, &phone);
    assert_eq!(s3.received.applied, 0);
    assert_eq!(c3.received.applied, 0);
    let (a, b) = (pc.vault(), phone.vault());
    assert_converged(&a, &b);
}

#[test]
fn trois_appareils_la_liste_et_les_donnees_se_propagent() {
    let (pc, phone, tablet) = (Tmp::new("three_pc"), Tmp::new("three_phone"), Tmp::new("three_tab"));
    pair(&pc, &phone, "Téléphone");
    pair(&pc, &tablet, "Tablette");
    // Le téléphone apprend l'existence de la tablette via le PC…
    sync(&pc, &phone);
    {
        let v = phone.vault();
        assert_eq!(engine(&v).devices().unwrap().len(), 3);
        v.add_password(&Password::new("mail.fr", "me", "pw")).unwrap();
    }
    // …et peut alors se synchroniser directement avec elle.
    sync(&tablet, &phone);
    assert_eq!(tablet.vault().passwords_for("mail.fr").unwrap().len(), 1);
    sync(&pc, &tablet);
    assert_converged(&pc.vault(), &tablet.vault());
}

#[test]
fn mauvais_jeton_echoue_et_limite_les_essais() {
    let (pc, attacker) = (Tmp::new("tok_pc"), Tmp::new("tok_att"));
    let listener = TcpListener::bind((LOCAL, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (qr_tx, qr_rx) = mpsc::channel();
    let path = pc.0.clone();
    let h = thread::spawn(move || {
        let v = Vault::open(&path, "pw").unwrap();
        let e = engine(&v);
        let mut offer = Offer::new(&e, vec![LOCAL], port).unwrap();
        qr_tx.send(offer.encode()).unwrap();
        let r = pairing::host(&e, &listener, &mut offer, |_, _| panic!("aucun canal ne doit s'établir"));
        (r.map(|_| ()), offer.is_valid(), offer.failures(), engine(&v).devices().unwrap().len())
    });
    let qr = qr_rx.recv().unwrap();
    // L'attaquant connaît l'adresse et la clé du PC, pas le jeton.
    let mut raw = URL_SAFE_NO_PAD.decode(&qr["nyxpair:v1:".len()..]).unwrap();
    raw[1] ^= 0xFF;
    let forged = format!("nyxpair:v1:{}", URL_SAFE_NO_PAD.encode(&raw));
    let v = attacker.vault();
    let e = engine(&v);
    for _ in 0..pairing::MAX_ATTEMPTS {
        let err = pairing::accept(&forged, &e, "intrus", |_, _| true).unwrap_err();
        assert!(sync_err(&err).is_some(), "erreur typée attendue : {err:?}");
    }
    let (r, valid, failures, n) = h.join().unwrap();
    assert_eq!(sync_err(&r.unwrap_err()), Some(SyncError::TooManyAttempts));
    assert!(!valid, "offre invalidée");
    assert_eq!(failures, pairing::MAX_ATTEMPTS);
    assert_eq!(n, 1, "personne n'a été ajouté");
    // Même le vrai jeton ne sert plus.
    let err = pairing::accept(&qr, &e, "tardif", |_, _| true).unwrap_err();
    assert!(err.chain().any(|c| c.to_string().contains("connexion")) || sync_err(&err).is_some());
    assert_eq!(engine(&v).devices().unwrap().len(), 1);
}

#[test]
fn offre_expiree_refusee() {
    let pc = Tmp::new("exp_pc");
    let v = pc.vault();
    let e = engine(&v);
    let listener = TcpListener::bind((LOCAL, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut offer = Offer::with_ttl(&e, vec![LOCAL], port, Duration::ZERO).unwrap();
    assert!(!offer.is_valid());
    let err = pairing::host(&e, &listener, &mut offer, |_, _| true).unwrap_err();
    assert_eq!(sync_err(&err), Some(SyncError::OfferExpired));
    let phone = Tmp::new("exp_phone");
    let pv = phone.vault();
    let err = pairing::accept(&offer.encode(), &engine(&pv), "x", |_, _| true).unwrap_err();
    assert_eq!(sync_err(&err), Some(SyncError::OfferExpired));
    // TTL plafonné à 5 min.
    let o = Offer::with_ttl(&e, vec![LOCAL], port, Duration::from_secs(3600)).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert!(o.info().expires_at <= now + pairing::MAX_TTL.as_secs());
}

#[test]
fn refus_utilisateur_n_ajoute_personne() {
    let (pc, phone) = (Tmp::new("rej_pc"), Tmp::new("rej_phone"));
    let listener = TcpListener::bind((LOCAL, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (qr_tx, qr_rx) = mpsc::channel();
    let path = pc.0.clone();
    let h = thread::spawn(move || {
        let v = Vault::open(&path, "pw").unwrap();
        let e = engine(&v);
        let mut offer = Offer::new(&e, vec![LOCAL], port).unwrap();
        qr_tx.send(offer.encode()).unwrap();
        let r = pairing::host(&e, &listener, &mut offer, |_, _| true);
        (r.map(|_| ()), e.devices().unwrap().len())
    });
    let qr = qr_rx.recv().unwrap();
    let v = phone.vault();
    let e = engine(&v);
    let before = e.profile().unwrap().discovery_tag();
    let err = pairing::accept(&qr, &e, "Pixel", |_, _| false).unwrap_err();
    assert_eq!(sync_err(&err), Some(SyncError::Rejected));
    let (r, n) = h.join().unwrap();
    assert_eq!(sync_err(&r.unwrap_err()), Some(SyncError::Rejected));
    assert_eq!(n, 1);
    assert_eq!(e.profile().unwrap().discovery_tag(), before, "profil du téléphone inchangé");
}

#[test]
fn appareil_non_appaire_ou_revoque_rejete_en_kk() {
    let (pc, phone, stranger) = (Tmp::new("kk_pc"), Tmp::new("kk_phone"), Tmp::new("kk_str"));
    let (phone_dev, _) = pair(&pc, &phone, "Pixel");
    sync(&pc, &phone);

    // Un inconnu qui se fabrique un profil contenant la clé du PC : le PC ne
    // connaît pas SA clé → handshake KK impossible.
    {
        let listener = TcpListener::bind((LOCAL, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let path = pc.0.clone();
        let h = thread::spawn(move || {
            let v = Vault::open(&path, "pw").unwrap();
            session::serve_one(&engine(&v), &listener).map(|_| ())
        });
        let sv = stranger.vault();
        let se = engine(&sv);
        // Sans pair connu : refus immédiat, sans connexion.
        assert_eq!(sync_err(&session::sync_with(&se, addr, None).unwrap_err()), Some(SyncError::UnknownPeer));
        // Avec la clé du PC « volée » dans son profil : le PC refuse.
        let pc_dev = engine(&pc.vault()).profile().unwrap().device(pc.vault().device_id()).unwrap().clone();
        assert!(inject_device(&sv, &se, pc_dev));
        let err = session::sync_with(&se, addr, None).unwrap_err();
        assert!(sync_err(&err).is_some(), "{err:#}");
        assert_eq!(sync_err(&h.join().unwrap().unwrap_err()), Some(SyncError::UnknownPeer));
        assert!(sv.passwords_for("x").unwrap().is_empty());
    }

    // Révocation du téléphone : rotation du secret, plus de session possible.
    let tag_before;
    {
        let v = pc.vault();
        let e = engine(&v);
        tag_before = e.discovery_tag().unwrap();
        e.revoke(&phone_dev.device_id).unwrap();
        assert_ne!(e.discovery_tag().unwrap(), tag_before);
        assert_eq!(e.profile().unwrap().epoch, 1);
        assert!(e.revoke(v.device_id()).is_err());
    }
    let listener = TcpListener::bind((LOCAL, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let path = pc.0.clone();
    let h = thread::spawn(move || {
        let v = Vault::open(&path, "pw").unwrap();
        session::serve_one(&engine(&v), &listener).map(|_| ())
    });
    let v = phone.vault();
    assert!(session::sync_with(&engine(&v), addr, None).is_err());
    assert_eq!(sync_err(&h.join().unwrap().unwrap_err()), Some(SyncError::UnknownPeer));
    // Le téléphone garde l'ancien secret : il ne reconnaît plus l'étiquette du PC.
    assert_eq!(engine(&v).discovery_tag().unwrap(), tag_before);
}

/// Ajoute `d` au profil local (simulation d'un attaquant qui connaît une
/// clé publique — information non secrète).
fn inject_device(v: &Vault, e: &SyncEngine, d: Device) -> bool {
    let mut p = e.profile().unwrap();
    let mut other = p.clone();
    other.devices.push(d);
    let changed = p.merge(&other);
    v.set_sync_meta("sync.profile", &serde_json::to_vec(&p).unwrap()).unwrap();
    changed
}

#[test]
fn manual_ip_cible_le_bon_pair_parmi_plusieurs() {
    // PC appairé à deux appareils ; le téléphone sert, la tablette n'est pas
    // en ligne : `sync_with(addr, None)` essaie les clés et trouve le bon.
    let (pc, phone, tablet) = (Tmp::new("man_pc"), Tmp::new("man_phone"), Tmp::new("man_tab"));
    pair(&pc, &tablet, "Tablette");
    pair(&pc, &phone, "Téléphone");
    let (s, c) = sync(&phone, &pc);
    assert_eq!(c.peer.device_id, phone.vault().device_id());
    assert_eq!(s.peer.device_id, pc.vault().device_id());
}

#[test]
fn gros_volume_lots_multiples_et_messages_decoupes() {
    let (pc, phone) = (Tmp::new("big_pc"), Tmp::new("big_phone"));
    pair(&pc, &phone, "Pixel");
    {
        let v = pc.vault();
        let items: Vec<Bookmark> =
            (0..1500).map(|i| Bookmark::new(format!("https://site{i}.org"), "t".repeat(1000))).collect();
        v.replace_bookmarks(&items).unwrap();
        v.set_setting("gros", &"x".repeat(200_000)).unwrap(); // > une trame Noise
    }
    let (s, _) = sync(&phone, &pc);
    assert_eq!(s.received.applied, 1500 + 1);
    let (a, b) = (pc.vault(), phone.vault());
    assert_eq!(b.list_bookmarks().unwrap().len(), 1500);
    assert_eq!(b.setting("gros").unwrap().map(|s| s.len()), Some(200_000));
    assert_converged(&a, &b);
}

#[cfg(feature = "mdns")]
#[test]
#[ignore = "multicast LAN requis : cargo test -p nyx-sync -- --ignored"]
fn mdns_annonce_et_decouverte() {
    let (pc, phone) = (Tmp::new("mdns_pc"), Tmp::new("mdns_phone"));
    pair(&pc, &phone, "Pixel");
    let v = pc.vault();
    let ad = nyx_sync::discovery::Advertiser::start(&engine(&v), 47_999).unwrap();
    let pv = phone.vault();
    let found = nyx_sync::discovery::browse(&engine(&pv), Duration::from_secs(5)).unwrap();
    drop(ad);
    assert!(found.iter().any(|f| f.device.device_id == v.device_id() && f.addrs.iter().all(|a| a.port() == 47_999)));
}
