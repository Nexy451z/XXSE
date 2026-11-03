//! Rotation rules for save snapshots.

use mhnx_core::snapshot;

#[test]
fn stamps_are_sortable_and_fixed_width() {
    // 2026-11-04 01:28:12 UTC, checked against python's datetime.
    assert_eq!(snapshot::stamp(1_793_755_692), "20261104-012812");
    let a = snapshot::stamp(1_000_000_000);
    let b = snapshot::stamp(1_100_000_000);
    assert_eq!(a.len(), b.len());
    assert!(a < b, "string order must match time order");
}

#[test]
fn names_carry_the_stem_and_sort_chronologically() {
    assert_eq!(
        snapshot::name("system", 1_793_755_692),
        "system-20261104-012812.sav"
    );
}

#[test]
fn rotation_drops_the_oldest_first() {
    let names = vec![
        String::from("system-20260101-000000.sav"),
        String::from("system-20260303-000000.sav"),
        String::from("system-20260202-000000.sav"),
        String::from("other-20250101-000000.sav"),
    ];
    // Keeping two of the three system snapshots drops the January one only.
    let drop = snapshot::older_than(&names, "system-", 2);
    assert_eq!(drop, vec![0]);
    // The strictly different stem is never touched.
    assert!(!drop.contains(&3));
    // Keeping none drops all three.
    assert_eq!(snapshot::older_than(&names, "system-", 0).len(), 3);
    // Keeping more than there are drops nothing.
    assert!(snapshot::older_than(&names, "system-", 9).is_empty());
}

#[test]
fn rotation_sorts_before_cutting() {
    // Given out of order, the oldest is still the one dropped.
    let names = vec![
        String::from("s-20260505-000000.sav"),
        String::from("s-20260101-000000.sav"),
        String::from("s-20260303-000000.sav"),
    ];
    assert_eq!(snapshot::older_than(&names, "s-", 1), vec![1, 2]);
}