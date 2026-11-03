//! CSV export and import: the round trip, the quoting, and the report.
//!
//! Every test builds its own save with empty_switch_save(); the real save under
//! C:\anyWorkspace\nx\jksv_ready is never opened here, let alone written.

use mhnx_core::csvout::{self, CsvError, EquipmentRow};
use mhnx_core::equip;
use mhnx_core::equipops;
use mhnx_core::itembox::{ItemSlot, TOTAL_ITEM_SLOTS};
use mhnx_core::save::{empty_switch_save, Save};

/// A box with a stack at each end, so a file that only listed the used records
/// would lose the place of everything after the first hole.
fn items_with_some() -> Vec<ItemSlot> {
    let mut items = vec![ItemSlot::default(); TOTAL_ITEM_SLOTS];
    items[0] = ItemSlot { id: 1, count: 99 };
    items[1] = ItemSlot { id: 2638, count: 5 };
    items[17] = ItemSlot { id: 4095, count: 7 };
    items[2299] = ItemSlot { id: 12, count: 42 };
    items
}

/// A save whose first three equipment records hold something, with a layered
/// id and decorations so every column has a value that could be lost.
fn equipment_save() -> Save {
    let mut save = empty_switch_save();
    assert_eq!(equipops::create(&mut save, 0, 7, 26, 5), Ok(0));
    {
        let rec = save.equipment_record_mut(0, 0).expect("record 0");
        assert!(equip::set_transmog(rec, 300, 4));
        assert!(equip::set_decoration(rec, 0, 2638));
        assert!(equip::set_decoration(rec, 1, 2639));
    }
    assert_eq!(equipops::create(&mut save, 0, 1, 100, 9), Ok(1));
    {
        let rec = save.equipment_record_mut(0, 1).expect("record 1");
        assert!(equip::set_decoration(rec, 2, 2640));
    }
    assert_eq!(equipops::create(&mut save, 0, 6, 12, 1), Ok(2));
    save
}

#[test]
fn items_round_trip_reproduces_the_box() {
    let mut save = empty_switch_save();
    let items = items_with_some();
    save.write_item_box(0, &items).expect("the box is writable");

    let csv = csvout::items_to_csv(&save, 0);
    assert!(csv.starts_with("slot,id,count\r\n"));
    assert_eq!(csv.lines().count(), TOTAL_ITEM_SLOTS + 1);
    // Every line ends with CRLF: no line break without its carriage return.
    let bytes = csv.as_bytes();
    let mut lone = 0;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' && (i == 0 || bytes[i - 1] != b'\r') {
            lone += 1;
        }
    }
    assert_eq!(lone, 0, "every line break is CRLF");

    let rows = csvout::parse_items(&csv).expect("the file it wrote reads back");
    assert_eq!(rows.len(), TOTAL_ITEM_SLOTS);
    assert_eq!(rows[0], (1, 99));
    assert_eq!(rows[1], (2638, 5));
    assert_eq!(rows[17], (4095, 7));
    assert_eq!(rows[2299], (12, 42));

    let mut fresh = empty_switch_save();
    let report = csvout::apply_items(&mut fresh, 0, &rows);
    assert_eq!(report.applied, TOTAL_ITEM_SLOTS);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.clamped, 0);
    assert_eq!(fresh.read_item_box(0).expect("the box reads"), items);
    assert_eq!(csvout::items_to_csv(&fresh, 0), csv);
}

#[test]
fn equipment_round_trip_reproduces_the_box() {
    let mut save = equipment_save();
    let before = save.equipment_box(0).expect("a box").to_vec();
    let csv = csvout::equipment_to_csv(&save, 0);
    assert!(csv.starts_with("index,kind,id,level,transmog,deco1,deco2,deco3\r\n"));
    assert_eq!(csv.lines().count(), 4, "header plus the three used records");

    let rows = csvout::parse_equipment(&csv).expect("the file it wrote reads back");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].index, 0);
    assert_eq!(rows[0].kind, 7);
    assert_eq!(rows[0].id, 26);
    assert_eq!(rows[0].level, 5);
    assert_eq!(rows[0].transmog, 300);
    assert_eq!(rows[0].deco, [2638, 2639, 0]);

    // Rub out every value the file carries, then put it back.
    for index in 0..3 {
        let rec = save.equipment_record_mut(0, index).expect("record");
        assert!(equip::set_level(rec, 1));
        for socket in 0..3 {
            assert!(equip::set_decoration(rec, socket, 0));
        }
    }
    let report = csvout::apply_equipment(&mut save, 0, &rows);
    assert_eq!(report.applied, 3);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.clamped, 0);
    assert_eq!(save.equipment_box(0).expect("a box"), &before[..]);
    assert_eq!(csvout::equipment_to_csv(&save, 0), csv);
}

#[test]
fn a_row_whose_type_changed_replaces_the_record() {
    let mut save = equipment_save();
    let csv = csvout::equipment_to_csv(&save, 0);
    assert!(equipops::replace(&mut save, 0, 0, 3, 77, 2).is_ok());
    assert_ne!(csvout::equipment_to_csv(&save, 0), csv);

    let rows = csvout::parse_equipment(&csv).expect("reads back");
    let report = csvout::apply_equipment(&mut save, 0, &rows);
    assert_eq!(report.applied, 3);
    assert_eq!(report.skipped, 0);
    assert_eq!(csvout::equipment_to_csv(&save, 0), csv);
}

#[test]
fn importing_into_an_empty_box_creates_the_records() {
    let mut save = empty_switch_save();
    let csv = "index,kind,id,level,transmog,deco1,deco2,deco3\r\n\
               0,7,26,5,300,2638,0,0\r\n\
               1,1,100,9,0,0,0,2640\r\n";
    let rows = csvout::parse_equipment(csv).expect("the file parses");
    let report = csvout::apply_equipment(&mut save, 0, &rows);
    assert_eq!(report.applied, 2);
    assert_eq!(report.skipped, 0);
    assert_eq!(csvout::equipment_to_csv(&save, 0), csv);
}

#[test]
fn a_row_for_an_empty_slot_is_created_and_says_where() {
    let mut save = empty_switch_save();
    let rows = csvout::parse_equipment(
        "index,kind,id,level,transmog,deco1,deco2,deco3\r\n5,7,26,5,0,0,0,0\r\n",
    )
    .expect("parses");
    let report = csvout::apply_equipment(&mut save, 0, &rows);
    assert_eq!(report.applied, 1);
    assert!(
        report.messages.iter().any(|m| m.contains("#0")),
        "the message says where the record went: {:?}",
        report.messages
    );
    assert_eq!(save.equipment_at(0, 0).map(|e| e.kind), Some(7));
}

#[test]
fn a_row_with_no_type_is_skipped() {
    let mut save = equipment_save();
    let before = save.equipment_box(0).expect("box").to_vec();
    let rows = vec![EquipmentRow {
        index: 0,
        kind: 0,
        id: 0,
        level: 1,
        transmog: 0,
        deco: [0; 3],
    }];
    let report = csvout::apply_equipment(&mut save, 0, &rows);
    assert_eq!(report.applied, 0);
    assert_eq!(report.skipped, 1);
    assert_eq!(save.equipment_box(0).expect("box"), &before[..]);
}

#[test]
fn quoted_cells_survive_the_reader() {
    let typed = ["a,b", "c\"d", "e\nf", "g\r\nh", "  spaced  ", ""];
    let text = csvout::csv_line(&typed);
    let rows = csvout::parse_rows(&text).expect("the line it wrote reads back");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].line, 1);
    let back: Vec<&str> = rows[0].fields.iter().map(|s| s.as_str()).collect();
    assert_eq!(back, typed);

    // A quoted cell holding line breaks moves the line counter, so the record
    // after it is reported on the line a spreadsheet would show.
    let mut both = text.clone();
    both.push_str(&csvout::csv_line(&["tail"]));
    let rows = csvout::parse_rows(&both).expect("two records read");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[1].line, 4,
        "two breaks inside the cells plus the row's own CRLF"
    );
}

#[test]
fn a_quoted_header_and_quoted_numbers_parse() {
    let csv = "\u{feff}\"slot\",\"id\",\"count\"\r\n\"0\",\"2638\",\"5\"\r\n";
    assert_eq!(csvout::parse_items(csv).expect("parses"), vec![(2638, 5)]);
}

#[test]
fn a_broken_row_reports_its_line() {
    let err = csvout::parse_items("slot,id,count\r\n0,5,3\r\n1,abc,4\r\n")
        .expect_err("the third line is not a number");
    assert_eq!(
        err,
        CsvError::NotANumber {
            line: 3,
            column: 2,
            value: String::from("abc"),
        }
    );
    let shown = err.to_string();
    assert!(
        shown.contains("3 行目"),
        "the message names the line: {}",
        shown
    );
}

#[test]
fn a_missing_equipment_column_reports_its_line() {
    let csv = "index,kind,id,level,transmog,deco1,deco2,deco3\r\n\
               0,7,26,5,0,0,0,0\r\n\
               1,7,27,5\r\n";
    let err = csvout::parse_equipment(csv).expect_err("the third line stops early");
    assert_eq!(
        err,
        CsvError::MissingColumn {
            line: 3,
            column: 5,
            name: "transmog",
        }
    );
    assert!(err.to_string().contains("3 行目"));
}

#[test]
fn an_unterminated_quote_reports_the_record_line() {
    let err =
        csvout::parse_items("slot,id,count\r\n0,5,\"3\r\n").expect_err("the quote never closes");
    assert_eq!(err, CsvError::UnterminatedQuote { line: 2 });
    assert!(err.to_string().contains("2 行目"));
}

#[test]
fn an_id_that_does_not_fit_twelve_bits_is_refused() {
    let err =
        csvout::parse_items("slot,id,count\r\n0,5000,3\r\n").expect_err("4095 is the largest id");
    assert_eq!(
        err,
        CsvError::OutOfRange {
            line: 2,
            column: 2,
            value: 5000,
            min: 0,
            max: 4095,
        }
    );
}

#[test]
fn a_count_above_99_is_clamped_and_reported() {
    let mut save = empty_switch_save();
    let rows = [(1u16, 150u16), (2, 99), (3, 100)];
    let report = csvout::apply_items(&mut save, 0, &rows);
    assert_eq!(report.applied, 3);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.clamped, 2);
    assert!(
        report.messages.iter().any(|m| m.contains("99")),
        "the message says what it rounded to: {:?}",
        report.messages
    );

    let items = save.read_item_box(0).expect("the box reads");
    assert_eq!(items[0], ItemSlot { id: 1, count: 99 });
    assert_eq!(items[1], ItemSlot { id: 2, count: 99 });
    assert_eq!(items[2], ItemSlot { id: 3, count: 99 });

    // The reader keeps what was typed; only apply rounds, and it reports it.
    assert_eq!(
        csvout::parse_items("slot,id,count\r\n0,1,150\r\n").expect("parses"),
        vec![(1, 150)]
    );
}

#[test]
fn extra_columns_are_ignored() {
    assert_eq!(
        csvout::parse_items("slot,id,count,note\r\n0,2638,5,hello\r\n").expect("parses"),
        vec![(2638, 5)]
    );
    let rows = csvout::parse_equipment(
        "index,kind,id,level,transmog,deco1,deco2,deco3,note\r\n3,7,26,5,300,2638,0,0,x\r\n",
    )
    .expect("parses");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].index, 3);
    assert_eq!(rows[0].kind, 7);
    assert_eq!(rows[0].deco, [2638, 0, 0]);
}

#[test]
fn a_line_with_nothing_on_it_is_not_a_record() {
    let rows = csvout::parse_rows("\r\n\r\n1,2,3\r\n\r\n").expect("parses");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].line, 3);
    assert_eq!(rows[0].fields, vec!["1", "2", "3"]);
}

#[test]
fn an_unknown_slot_is_reported_and_not_written() {
    let mut save = empty_switch_save();
    let report = csvout::apply_items(&mut save, 3, &[(1, 1)]);
    assert_eq!(report.applied, 0);
    assert_eq!(report.skipped, 1);
    assert!(!report.messages.is_empty());
    assert_eq!(csvout::items_to_csv(&save, 3), "slot,id,count\r\n");
    assert_eq!(
        csvout::equipment_to_csv(&save, 3),
        "index,kind,id,level,transmog,deco1,deco2,deco3\r\n"
    );
}

#[test]
fn an_empty_file_does_not_wipe_the_box() {
    let mut save = empty_switch_save();
    let items = items_with_some();
    save.write_item_box(0, &items).expect("writable");
    let report = csvout::apply_items(&mut save, 0, &[]);
    assert_eq!(report.applied, 0);
    assert!(!report.messages.is_empty());
    assert_eq!(save.read_item_box(0).expect("reads"), items);
}
