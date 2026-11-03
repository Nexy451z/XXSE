//! Proving a write landed, and reading the shape of a container.

use mhnx_core::save::{self, BODY_SIZE, SWITCH_SIZE};
use mhnx_core::verify;

fn container(name: &str) -> Vec<u8> {
    let body = vec![0u8; BODY_SIZE];
    let mut out = save::switch_header(name, body.len()).to_vec();
    out.extend_from_slice(&body);
    out
}

#[test]
fn identical_bytes_report_nothing() {
    let a = vec![1u8, 2, 3, 4];
    let r = verify::compare(&a, &a);
    assert!(r.is_identical());
    assert!(r.summary().contains("一致"));
}

#[test]
fn a_changed_byte_is_named() {
    let a = vec![1u8, 2, 3, 4];
    let mut b = a.clone();
    b[2] = 9;
    let r = verify::compare(&a, &b);
    assert!(!r.is_identical());
    assert_eq!(r.differing, vec![(2, 3, 9)]);
    assert!(r.examples(3).contains("0x2"));
}

#[test]
fn a_short_write_is_reported_as_a_length_change() {
    let a = vec![0u8; 10];
    let b = vec![0u8; 8];
    let r = verify::compare(&a, &b);
    assert_eq!(r.length_change, Some((10, 8)));
    assert!(!r.is_identical());
}

#[test]
fn a_healthy_container_has_no_findings() {
    let bytes = container("system");
    assert_eq!(bytes.len(), SWITCH_SIZE);
    assert!(verify::check_all(&bytes, "system").is_empty());
}

#[test]
fn the_wrong_name_is_caught_by_the_header_hash() {
    let bytes = container("system");
    // Same bytes, wrong name: this is what a copy of one file over another
    // looks like, and only the header hash can tell.
    let f = verify::check_header_name(&bytes, "system_backup");
    assert_eq!(f, vec![verify::Finding::HeaderNameHash]);
    assert!(f[0].text().contains("名前ハッシュ"));
}

#[test]
fn a_bad_length_is_caught() {
    let bytes = vec![0u8; 100];
    let f = verify::check_shape(&bytes);
    assert_eq!(f, vec![verify::Finding::UnexpectedLength(100)]);
}

#[test]
fn a_bare_3ds_body_is_always_the_right_shape() {
    let bytes = vec![0u8; BODY_SIZE];
    assert!(verify::check_all(&bytes, "system").is_empty());
}