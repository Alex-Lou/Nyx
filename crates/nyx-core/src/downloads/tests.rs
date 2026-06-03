//! Tests downloads — modèle, transitions, hostiles, stress.

use super::*;
use crate::download_policy::Kind;

fn mk() -> DownloadStore { DownloadStore::new() }

// ─── add / order ───────────────────────────────────────────────────────────

#[test]
fn add_inserts_at_head() {
    let mut s = mk();
    let id1 = s.add("a.pdf".into(), "/dl/a.pdf".into(), Kind::Safe, "https://e.com".into(), 1);
    let id2 = s.add("b.pdf".into(), "/dl/b.pdf".into(), Kind::Safe, "https://e.com".into(), 2);
    assert_eq!(s.entries()[0].id, id2, "le plus récent en tête");
    assert_eq!(s.entries()[1].id, id1);
}

#[test]
fn ids_are_monotonic_unique() {
    let mut s = mk();
    let mut last = 0;
    for i in 0..100 {
        let id = s.add(format!("f{i}"), "/x".into(), Kind::Safe, "x".into(), 0);
        assert!(id.as_u64() >= last);
        last = id.as_u64() + 1;
    }
}

#[test]
fn id_after_remove_not_reused() {
    let mut s = mk();
    let id1 = s.add("a".into(), "/a".into(), Kind::Safe, "x".into(), 0);
    s.remove(id1);
    let id2 = s.add("b".into(), "/b".into(), Kind::Safe, "x".into(), 0);
    assert_ne!(id1, id2);
}

// ─── remove / clear ────────────────────────────────────────────────────────

#[test]
fn remove_known_returns_true() {
    let mut s = mk();
    let id = s.add("a".into(), "/a".into(), Kind::Safe, "x".into(), 0);
    assert!(s.remove(id));
    assert!(s.is_empty());
}

#[test]
fn remove_unknown_returns_false() {
    let mut s = mk();
    assert!(!s.remove(DownloadId::from_u64(999)));
}

#[test]
fn clear_all_empties_list() {
    let mut s = mk();
    for i in 0..5 {
        s.add(format!("f{i}"), "/x".into(), Kind::Safe, "x".into(), 0);
    }
    s.clear_all();
    assert!(s.is_empty());
}

// ─── status / transitions ──────────────────────────────────────────────────

#[test]
fn initial_status_is_in_progress() {
    let mut s = mk();
    let id = s.add("a".into(), "/a".into(), Kind::Safe, "x".into(), 0);
    assert!(matches!(s.entries()[0].status,
        DownloadStatus::InProgress { received: 0, total: None }));
    let _ = id;
}

#[test]
fn update_progress_changes_received_and_total() {
    let mut s = mk();
    let id = s.add("a".into(), "/a".into(), Kind::Safe, "x".into(), 0);
    s.update_progress(id, 500, Some(1000));
    assert_eq!(s.entries()[0].status,
        DownloadStatus::InProgress { received: 500, total: Some(1000) });
}

#[test]
fn fraction_is_correct() {
    let p = DownloadStatus::InProgress { received: 250, total: Some(1000) };
    assert_eq!(p.fraction(), Some(0.25));
    assert_eq!(DownloadStatus::Completed.fraction(), Some(1.0));
    assert_eq!(DownloadStatus::Cancelled.fraction(), None);
    assert_eq!(DownloadStatus::InProgress { received: 0, total: None }.fraction(), None);
}

#[test]
fn mark_completed_sets_finished_and_path() {
    let mut s = mk();
    let id = s.add("a.tmp".into(), "/dl/a.tmp".into(), Kind::Safe, "x".into(), 0);
    s.mark_completed(id, Some("/dl/a.pdf".into()), 100);
    let e = &s.entries()[0];
    assert_eq!(e.status, DownloadStatus::Completed);
    assert_eq!(e.dest_path, "/dl/a.pdf");
    assert_eq!(e.finished_at_unix, 100);
}

#[test]
fn mark_completed_without_renaming_keeps_path() {
    let mut s = mk();
    let id = s.add("a.pdf".into(), "/dl/a.pdf".into(), Kind::Safe, "x".into(), 0);
    s.mark_completed(id, None, 100);
    assert_eq!(s.entries()[0].dest_path, "/dl/a.pdf");
}

#[test]
fn mark_cancelled_sets_status_and_time() {
    let mut s = mk();
    let id = s.add("a".into(), "/a".into(), Kind::Safe, "x".into(), 0);
    s.mark_cancelled(id, 50);
    assert_eq!(s.entries()[0].status, DownloadStatus::Cancelled);
    assert_eq!(s.entries()[0].finished_at_unix, 50);
}

#[test]
fn mark_failed_stores_reason() {
    let mut s = mk();
    let id = s.add("a".into(), "/a".into(), Kind::Safe, "x".into(), 0);
    s.mark_failed(id, "net error".into(), 50);
    assert_eq!(s.entries()[0].status,
        DownloadStatus::Failed { reason: "net error".into() });
}

#[test]
fn terminal_status_cannot_be_overridden() {
    let mut s = mk();
    let id = s.add("a".into(), "/a".into(), Kind::Safe, "x".into(), 0);
    s.mark_completed(id, None, 10);
    s.mark_cancelled(id, 20); // no-op
    s.mark_failed(id, "x".into(), 30); // no-op
    assert_eq!(s.entries()[0].status, DownloadStatus::Completed);
    assert_eq!(s.entries()[0].finished_at_unix, 10);
}

#[test]
fn progress_on_terminal_is_noop() {
    let mut s = mk();
    let id = s.add("a".into(), "/a".into(), Kind::Safe, "x".into(), 0);
    s.mark_completed(id, None, 10);
    s.update_progress(id, 999, Some(1000));
    assert_eq!(s.entries()[0].status, DownloadStatus::Completed);
}

#[test]
fn updates_on_unknown_id_are_noop() {
    let mut s = mk();
    let unknown = DownloadId::from_u64(42);
    s.update_progress(unknown, 100, None);
    s.mark_completed(unknown, None, 0);
    s.mark_cancelled(unknown, 0);
    s.mark_failed(unknown, "x".into(), 0);
    assert!(s.is_empty());
}

// ─── prune ─────────────────────────────────────────────────────────────────

#[test]
fn prune_completed_keeps_in_progress() {
    let mut s = mk();
    let a = s.add("a".into(), "/a".into(), Kind::Safe, "x".into(), 0);
    let b = s.add("b".into(), "/b".into(), Kind::Safe, "x".into(), 0);
    s.mark_completed(a, None, 1);
    s.prune_completed();
    assert_eq!(s.entries().len(), 1);
    assert_eq!(s.entries()[0].id, b);
}

// ─── hostile / sanity ──────────────────────────────────────────────────────

#[test]
fn empty_filename_accepted_for_layering() {
    // Le browser SANITIZE déjà via download_policy::analyze. Le store ne
    // valide pas — il est juste un conteneur — mais ne doit pas paniquer.
    let mut s = mk();
    let _ = s.add("".into(), "".into(), Kind::Unknown, "".into(), 0);
    assert_eq!(s.len(), 1);
}

#[test]
fn many_status_transitions_no_panic() {
    let mut s = mk();
    for _ in 0..200 {
        let id = s.add("f".into(), "/f".into(), Kind::Safe, "x".into(), 0);
        s.update_progress(id, 1, Some(10));
        s.mark_completed(id, None, 1);
    }
}

// ─── cap MAX_ENTRIES ───────────────────────────────────────────────────────

#[test]
fn cap_holds_at_max_entries() {
    let mut s = mk();
    for i in 0..(super::store::MAX_ENTRIES + 200) {
        let id = s.add(format!("f{i}"), "/x".into(), Kind::Safe, "x".into(), i as i64);
        // Marque immédiatement comme terminal pour permettre l'éviction.
        s.mark_completed(id, None, i as i64);
    }
    assert!(s.len() <= super::store::MAX_ENTRIES,
        "cap violé : len={} > {}", s.len(), super::store::MAX_ENTRIES);
}

#[test]
fn cap_evicts_oldest_terminal_first() {
    let mut s = mk();
    // Quelques in-progress en tête de timeline (deviennent les plus vieux
    // après les nombreux ajouts qui suivent).
    let preserved_ids: Vec<_> = (0..3).map(|i|
        s.add(format!("keep{i}"), "/x".into(), Kind::Archive, "x".into(), i)
    ).collect();

    // Sature au-delà du cap avec des terminaux.
    let extra = super::store::MAX_ENTRIES + 50;
    for i in 0..extra {
        let id = s.add(format!("f{i}"), "/x".into(), Kind::Safe, "x".into(), 1000 + i as i64);
        s.mark_completed(id, None, 1000 + i as i64);
    }

    // Les in-progress préservés DOIVENT toujours être là — terminaux évincés d'abord.
    for kid in &preserved_ids {
        assert!(s.entries().iter().any(|e| e.id == *kid),
            "in-progress {kid} évincé alors qu'il restait des terminaux");
    }
    assert!(s.len() <= super::store::MAX_ENTRIES);
}

#[test]
fn cap_evicts_in_progress_under_extreme_saturation() {
    let mut s = mk();
    // MAX_ENTRIES + 1 in-progress : pas un seul terminal disponible.
    for i in 0..super::store::MAX_ENTRIES {
        s.add(format!("f{i}"), "/x".into(), Kind::Safe, "x".into(), i as i64);
    }
    assert_eq!(s.len(), super::store::MAX_ENTRIES);
    let oldest_before: Vec<_> = s.entries().iter().rev().take(3)
        .map(|e| e.id).collect();

    // Un de plus : doit évincer le plus ancien in-progress.
    let new_id = s.add("new".into(), "/x".into(), Kind::Safe, "x".into(), 99_999);
    assert_eq!(s.len(), super::store::MAX_ENTRIES);
    assert!(s.entries().iter().any(|e| e.id == new_id));
    // Le plus vieux d'avant n'est plus là.
    assert!(!s.entries().iter().any(|e| e.id == oldest_before[0]));
}

// ─── stress ────────────────────────────────────────────────────────────────

/// 10 000 opérations mêlées. Pas de panic, invariants tenus.
#[test]
fn stress_10k_mixed_ops() {
    let mut s = mk();
    let mut ids: Vec<DownloadId> = Vec::new();
    let kinds = [Kind::Safe, Kind::Archive, Kind::Executable, Kind::ActiveDoc, Kind::Unknown];
    for i in 0..10_000 {
        match i % 7 {
            0..=2 => {
                let id = s.add(
                    format!("f{i}"), format!("/d/f{i}"),
                    kinds[i % kinds.len()], "https://x".into(), i as i64,
                );
                ids.push(id);
            }
            3 if !ids.is_empty() => {
                let id = ids[i % ids.len()];
                s.update_progress(id, (i * 13) as u64, Some(100_000));
            }
            4 if !ids.is_empty() => {
                let id = ids[i % ids.len()];
                s.mark_completed(id, None, i as i64);
            }
            5 if !ids.is_empty() => {
                let id = ids[(i / 2) % ids.len()];
                s.mark_cancelled(id, i as i64);
            }
            6 if !ids.is_empty() => {
                let id = ids[(i / 3) % ids.len()];
                let _ = s.remove(id);
            }
            _ => {}
        }
    }
    // Invariant : aucun id dupliqué.
    let mut seen = std::collections::HashSet::new();
    for e in s.entries() {
        assert!(seen.insert(e.id), "id dupliqué : {}", e.id);
    }
}

// ─── DownloadId helpers exposés pour les tests uniquement ──────────────────

impl DownloadId {
    /// Reconstruit un id depuis un `u64`. **Tests uniquement** — on ne veut
    /// pas exposer ça en API publique (sinon le compteur monotone du store
    /// peut être contourné).
    pub(crate) fn from_u64(n: u64) -> Self { Self(n) }
}
