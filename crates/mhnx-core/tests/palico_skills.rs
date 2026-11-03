//! Reading and editing the learned skills, support moves and tendencies of a
//! palico, against the real save.
//!
//! The sample character holds 17 palicos and, right after them, 24 for hire.
//! The asserts that pin exact ids use the owned records only, so the tests
//! speak about the same fixture the rest of the suite uses.

use std::path::PathBuf;

use mhnx_core::palico::{self, Palico};
use mhnx_core::save::Save;

fn real_save() -> PathBuf {
    // The fixed copy first: the Ryujinx save is live and the game writes to it.
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    if stable.exists() {
        return stable;
    }
    PathBuf::from(r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system")
}

fn load() -> Option<Save> {
    let p = real_save();
    if !p.exists() {
        return None;
    }
    Some(Save::from_bytes(&std::fs::read(&p).unwrap()).unwrap())
}

/// File offset of record 0 of slot 0 (the save carries a 36 byte header).
fn record0() -> usize {
    36 + 0x126474 + palico::OFF_ROSTER
}

fn changed(before: &[u8], after: &[u8]) -> Vec<usize> {
    (0..before.len())
        .filter(|i| before[*i] != after[*i])
        .collect()
}

#[test]
fn the_learned_lists_read_the_slots_their_length_bytes_count() {
    let Some(s) = load() else {
        return;
    };
    let used = palico::used(&s, 0);
    assert_eq!(used.len(), 17, "the sample character has seventeen palicos");
    for p in &used {
        assert!(
            (p.skill_len as usize) <= palico::LEARNED_SKILL_SLOTS,
            "palico {} stores skill length {}",
            p.index,
            p.skill_len
        );
        assert!(
            (p.move_len as usize) <= palico::LEARNED_MOVE_SLOTS,
            "palico {} stores move length {}",
            p.index,
            p.move_len
        );
        // A learned list is [ids][empty taught slots][fill]: an empty slot never
        // comes before a used one.
        let mut empty = false;
        for v in p.learned_skills.iter().take(p.learned_skill_slots()) {
            if *v == 0 || *v == palico::SKILL_EMPTY {
                empty = true;
            } else {
                assert!(!empty, "palico {} has a skill gap", p.index);
            }
        }
        let mut empty = false;
        for v in p.learned_moves.iter().take(p.learned_move_slots()) {
            if *v == 0 || *v == palico::MOVE_EMPTY {
                empty = true;
            } else {
                assert!(!empty, "palico {} has a move gap", p.index);
            }
        }
        // The ids stay inside the two tables, and 0 is never stored as a skill.
        for id in palico::learned_skills(p) {
            assert!(
                id <= palico::MAX_SKILL,
                "palico {} learns skill id {}",
                p.index,
                id
            );
        }
        for id in palico::support_moves(p) {
            assert!(
                id <= palico::MAX_MOVE,
                "palico {} learns move id {}",
                p.index,
                id
            );
        }
        // The free readers and the methods have to agree.
        assert_eq!(palico::learned_skills(p), p.learned_skills());
        assert_eq!(palico::support_moves(p), p.learned_moves());
        assert_eq!(palico::equipped_skills(p), p.skills());
        assert_eq!(palico::equipped_moves(p), p.moves());
        assert!(palico::type_of(p).is_some(), "palico {} tendency", p.index);
    }
}

#[test]
fn the_sample_palicos_read_their_skills_and_moves() {
    let Some(s) = load() else {
        return;
    };
    let roster = palico::roster(&s, 0);

    // Record 0: eight learned skills in a ten slot list (two taught slots still
    // empty), nine learned moves in an eleven slot list, two moves equipped.
    let p = &roster[0];
    assert_eq!(p.name, "ロッキー");
    assert_eq!(p.skill_len, 10);
    assert_eq!(p.move_len, 11);
    assert_eq!(palico::learned_skills(p), vec![5, 46, 80, 2, 8, 33, 47, 32]);
    assert_eq!(
        palico::support_moves(p),
        vec![3, 5, 9, 1, 45, 4, 23, 53, 32]
    );
    assert_eq!(palico::equipped_moves(p), vec![3, 5]);
    assert!(palico::equipped_skills(p).is_empty());

    // Record 37: nine learned skills and a full list - teaching filled the two
    // slots a fresh record leaves empty - with four of them equipped.
    let cat = roster.iter().find(|p| p.index == 37).unwrap();
    assert_eq!(cat.skill_len, 9);
    assert_eq!(
        palico::learned_skills(cat),
        vec![41, 43, 80, 40, 31, 79, 29, 42, 8]
    );
    assert_eq!(palico::equipped_skills(cat), vec![41, 43, 42, 29]);
    assert!(palico::equipped_skills(cat)
        .iter()
        .all(|id| palico::learned_skills(cat).contains(id)));
}

#[test]
fn the_one_inconsistent_palico_in_the_sample_is_reported() {
    let Some(s) = load() else {
        return;
    };
    let roster = palico::roster(&s, 0);
    // The sample really holds a record that equips skills it never learned
    // (three of them) and moves it never learned (six of the eight). It is the
    // only record that does, so the warning has to single it out.
    for p in roster.iter().filter(|p| !p.is_empty()) {
        let skills = palico::skill_warnings(&s, 0, p.index);
        let moves = palico::move_warnings(&s, 0, p.index);
        if p.index == 39 {
            assert_eq!(skills.len(), 3, "{:?}", skills);
            assert_eq!(moves.len(), 6, "{:?}", moves);
            assert!(skills.iter().all(|m| m.contains("装備スキル")));
            assert!(moves.iter().all(|m| m.contains("装備サポート行動")));
        } else {
            assert!(skills.is_empty(), "palico {}: {:?}", p.index, skills);
            assert!(moves.is_empty(), "palico {}: {:?}", p.index, moves);
        }
    }
    // An empty record has nothing to check.
    assert!(palico::skill_warnings(&s, 0, palico::SLOTS).is_empty());
}

#[test]
fn replacing_a_learned_skill_writes_one_byte() {
    let Some(mut s) = load() else {
        return;
    };
    let before = s.to_bytes();
    assert!(palico::set_skill(&mut s, 0, 0, 0, 1));
    let after = s.to_bytes();
    assert_eq!(
        changed(&before, &after),
        vec![record0() + palico::LEARNED_SKILLS],
        "only the skill slot may move"
    );
    let p = &palico::roster(&s, 0)[0];
    assert_eq!(palico::learned_skills(p)[0], 1);
    assert_eq!(p.skill_len, 10, "the list length does not move");
    assert!(palico::skill_warnings(&s, 0, 0).is_empty());

    // Out of range is refused: past the list, past the table, and no id 0.
    assert!(!palico::set_skill(&mut s, 0, 0, 10, 1));
    assert!(!palico::set_skill(&mut s, 0, 0, 0, palico::MAX_SKILL + 1));
    assert!(!palico::set_skill(&mut s, 0, 0, 0, 0));
    assert!(!palico::set_skill(&mut s, 0, palico::SLOTS, 0, 1));
    assert!(!palico::set_skill(&mut s, 3, 0, 0, 1));
    // Writing the id that is already there changes nothing.
    let before = s.to_bytes();
    assert!(palico::set_skill(&mut s, 0, 0, 0, 1));
    assert_eq!(changed(&before, &s.to_bytes()), Vec::<usize>::new());
}

#[test]
fn teaching_fills_the_empty_slots_the_list_already_counts() {
    let Some(mut s) = load() else {
        return;
    };
    let before = s.to_bytes();
    assert_eq!(palico::add_skill(&mut s, 0, 0, 7), Some(8));
    assert_eq!(
        changed(&before, &s.to_bytes()),
        vec![record0() + palico::LEARNED_SKILLS + 8]
    );
    let p = &palico::roster(&s, 0)[0];
    assert_eq!(p.skill_len, 10, "teaching fills a slot that is counted");
    assert_eq!(
        palico::learned_skills(p),
        vec![5, 46, 80, 2, 8, 33, 47, 32, 7]
    );

    // The same skill is not taught twice, and an id outside the table is not a
    // skill at all.
    assert_eq!(palico::add_skill(&mut s, 0, 0, 7), None);
    assert_eq!(palico::add_skill(&mut s, 0, 0, 0), None);
    assert_eq!(palico::add_skill(&mut s, 0, 0, palico::MAX_SKILL + 1), None);

    // The last empty slot, then the list is full within its length.
    assert_eq!(palico::add_skill(&mut s, 0, 0, 9), Some(9));
    assert_eq!(palico::add_skill(&mut s, 0, 0, 10), None);
    assert!(palico::skill_warnings(&s, 0, 0).is_empty());
}

#[test]
fn replacing_an_equipped_skill_unequips_it() {
    let Some(mut s) = load() else {
        return;
    };
    let base = 36 + s.slot_base(0).unwrap() as usize + palico::OFF_ROSTER + 37 * palico::RECORD;
    let before = s.to_bytes();
    // Record 37 equips 41, 43, 42 and 29; replace 41.
    assert!(palico::set_skill(&mut s, 0, 37, 0, 7));
    let after = s.to_bytes();
    let outside: Vec<usize> = changed(&before, &after)
        .into_iter()
        .filter(|i| *i < base || *i >= base + palico::RECORD)
        .collect();
    assert!(
        outside.is_empty(),
        "bytes outside the record: {:?}",
        outside
    );

    let p = &palico::roster(&s, 0)[37];
    assert_eq!(p.learned_skills()[0], 7);
    assert_eq!(
        palico::equipped_skills(p),
        vec![43, 42, 29],
        "the replaced skill leaves the equipped list, kept packed"
    );
    assert!(palico::skill_warnings(&s, 0, 37).is_empty());
}

#[test]
fn support_moves_are_edited_the_same_way() {
    let Some(mut s) = load() else {
        return;
    };
    let before = s.to_bytes();
    // Record 0 equips moves 3 and 5, and 3 is learned at slot 0.
    assert!(palico::set_move(&mut s, 0, 0, 0, 56));
    let p = &palico::roster(&s, 0)[0];
    assert_eq!(palico::support_moves(p)[0], 56);
    assert_eq!(palico::equipped_moves(p), vec![5]);
    assert_eq!(p.move_len, 11);
    assert!(palico::move_warnings(&s, 0, 0).is_empty());
    let outside: Vec<usize> = changed(&before, &s.to_bytes())
        .into_iter()
        .filter(|i| *i < record0() || *i >= record0() + palico::RECORD)
        .collect();
    assert!(
        outside.is_empty(),
        "bytes outside the record: {:?}",
        outside
    );

    // The first empty taught slot is index 9 (nine ids in an eleven slot list).
    assert_eq!(palico::add_move(&mut s, 0, 0, 6), Some(9));
    assert_eq!(palico::add_move(&mut s, 0, 0, 6), None);
    assert_eq!(palico::add_move(&mut s, 0, 0, palico::MAX_MOVE + 1), None);
    assert!(!palico::set_move(&mut s, 0, 0, 11, 1));
    assert!(!palico::set_move(&mut s, 0, 0, 0, 0));
    assert_eq!(palico::roster(&s, 0)[0].move_len, 11);
}

#[test]
fn the_tendencies_are_range_checked() {
    let Some(mut s) = load() else {
        return;
    };
    let before = s.to_bytes();
    assert!(palico::set_type(&mut s, 0, 0, 7));
    assert_eq!(
        changed(&before, &s.to_bytes()),
        vec![record0() + palico::SYSTEM]
    );
    let p = &palico::roster(&s, 0)[0];
    assert_eq!(p.system, 7);
    assert_eq!(p.system_name(), "ビースト");
    assert_eq!(palico::type_of(p), Some(7));
    assert!(!palico::set_type(&mut s, 0, 0, palico::SYSTEMS_COUNT));

    let before = s.to_bytes();
    assert!(palico::set_attack(&mut s, 0, 0, 5));
    assert_eq!(
        changed(&before, &s.to_bytes()),
        vec![record0() + palico::ATTACK]
    );
    let p = &palico::roster(&s, 0)[0];
    assert_eq!(palico::attack_of(p), Some(5));
    assert!(!palico::set_attack(&mut s, 0, 0, palico::MAX_ATTACK + 1));

    // A byte the game does not offer reads as None rather than a tendency.
    let mut odd = roster0(&s);
    odd.system = 9;
    assert_eq!(palico::type_of(&odd), None);
    odd.attack = 6;
    assert_eq!(palico::attack_of(&odd), None);
}

fn roster0(s: &Save) -> Palico {
    palico::roster(s, 0)[0].clone()
}
