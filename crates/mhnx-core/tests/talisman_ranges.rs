use mhnx_core::save::Save;

/// The sample save, or None when the machine does not have it.
fn real_save() -> Option<Save> {
    let mut p = std::path::PathBuf::from("C:/");
    for part in ["Users", "haruk", "AppData", "Roaming", "Ryujinx", "bis", "user", "save", "0000000000000001", "0"] {
        p.push(part);
    }
    p.push("system");
    if !p.exists() {
        return None;
    }
    Save::from_bytes(&std::fs::read(&p).ok()?).ok()
}


// ---------------------------------------------------------------------------
// The real talisman point ranges, now that the table indexing is known.
// ---------------------------------------------------------------------------

/// The reference editor reads the type from +0x02, maps it through
/// TALISMAN_RARITY and indexes the per-rarity table by skill id - 1.
/// Every talisman in the sample save must agree with that.
#[test]
fn every_real_talisman_is_inside_the_published_range() {
    let Some(src) = real_save() else { return };
    let mut checked = 0usize;
    let mut outliers = 0usize;
    let mut violations = Vec::new();
    for slot in 0..3 {
        let Some(b) = src.equipment_box(slot) else { continue };
        for i in 0..mhnx_core::charm::CHARM_COUNT {
            let o = i * mhnx_core::charm::CHARM_RECORD;
            let rec = &b[o..o + mhnx_core::charm::CHARM_RECORD];
            if !mhnx_core::charm::is_talisman(rec) {
                continue;
            }
            let ty = rec[0x02];
            // Type 0 is an outlier: TALISMAN_RARITY[0] is 0 but two real
            // records store rarity 0x63 with it, and the reference switch has
            // no case for 0 at all. Skip those rather than pretend.
            if ty == 0 {
                outliers += 1;
                continue;
            }
            assert_eq!(
                mhnx_core::charm::rarity_of(ty),
                Some(rec[0x12] as i32),
                "slot {} talisman {}: type {:#04x} and rarity {:#04x} disagree",
                slot,
                i,
                ty,
                rec[0x12]
            );
            for (skill, pts, second) in [
                (rec[0x0C], rec[0x0E] as i8, false),
                (rec[0x0D], rec[0x0F] as i8, true),
            ] {
                let Some(r) = mhnx_core::charm::allowed_points(ty, skill, second) else {
                    continue;
                };
                checked += 1;
                if pts < r.min || pts > r.max {
                    violations.push((slot, i, skill, pts, r.min, r.max));
                }
            }
        }
    }
    eprintln!(
        "checked {} skill values, {} outside the range, {} type-0 outliers",
        checked,
        violations.len(),
        outliers
    );
    eprintln!("first few: {:?}", &violations[..violations.len().min(5)]);
    assert!(checked > 500, "only {} skills were checked", checked);
    assert!(
        violations.is_empty(),
        "{} values fall outside the published range, first: {:?}",
        violations.len(),
        violations.first()
    );
}

#[test]
fn a_skill_the_table_forbids_reports_a_zero_range() {
    // Enduring talismans (type 0x0A) do not allow skill 1 at all.
    assert_eq!(
        mhnx_core::charm::allowed_points(0x0A, 1, false),
        Some(mhnx_core::charm::AllowedPoints { min: 0, max: 0 })
    );
}

#[test]
fn the_type_byte_maps_to_the_measured_rarities() {
    for (ty, rarity) in [(0x00u8, 0), (0x04, 98), (0x06, 99), (0x07, 99), (0x0A, 100)] {
        assert_eq!(
            mhnx_core::charm::rarity_of(ty),
            Some(rarity),
            "type {:#04x}",
            ty
        );
    }
    // Out of range must not panic.
    assert_eq!(mhnx_core::charm::rarity_of(200), None);
}