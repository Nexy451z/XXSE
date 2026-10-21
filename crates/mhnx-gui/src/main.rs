//! mhnxeditor - Monster Hunter XX save and quest editor (PC build).
//!
//! The binary is intentionally self contained: it embeds no game data of its
//! own beyond the item / equipment name tables, and it writes files only when
//! the user explicitly asks for it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use eframe::egui;

/// Candidate Japanese fonts, in order of preference.
///
/// egui ships with a Latin-only font, so Japanese text would render as tofu
/// without this. Plain .ttf files are tried before .ttc collections.
const JP_FONT_CANDIDATES: &[&str] = &[
    r"C:\Windows\Fonts\NotoSansJP-VF.ttf",
    r"C:\Windows\Fonts\BIZ-UDGothicR.ttc",
    r"C:\Windows\Fonts\meiryo.ttc",
    r"C:\Windows\Fonts\YuGothR.ttc",
    r"C:\Windows\Fonts\msgothic.ttc",
    r"C:\Windows\Fonts\msmincho.ttc",
];

/// The same muted palette the console build uses, so the two look like one
/// program. Values checked for WCAG AA against every surface they sit on.
fn apply_theme(ctx: &egui::Context) {
    use egui::{Color32, Stroke};
    let mut v = egui::Visuals::dark();
    let c = |r: u8, g: u8, b: u8| Color32::from_rgb(r, g, b);

    v.panel_fill = c(24, 24, 26);
    v.window_fill = c(28, 28, 32);
    v.extreme_bg_color = c(20, 20, 22);
    v.faint_bg_color = c(36, 36, 40);
    v.code_bg_color = c(24, 24, 27);

    v.widgets.noninteractive.bg_fill = c(30, 30, 33);
    v.widgets.noninteractive.weak_bg_fill = c(30, 30, 33);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c(58, 58, 65));
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c(228, 228, 231));

    v.widgets.inactive.bg_fill = c(52, 52, 57);
    v.widgets.inactive.weak_bg_fill = c(44, 44, 49);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, c(110, 110, 120));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, c(228, 228, 231));

    v.widgets.hovered.bg_fill = c(66, 66, 72);
    v.widgets.hovered.weak_bg_fill = c(58, 58, 63);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, c(161, 161, 170));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, c(240, 240, 243));

    v.widgets.active.bg_fill = c(80, 80, 88);
    v.widgets.active.weak_bg_fill = c(74, 74, 80);
    v.widgets.active.bg_stroke = Stroke::new(1.0, c(228, 228, 231));
    v.widgets.active.fg_stroke = Stroke::new(1.0, c(255, 255, 255));

    v.selection.bg_fill = c(58, 58, 64);
    v.selection.stroke = Stroke::new(1.0, c(228, 228, 231));
    v.hyperlink_color = c(161, 161, 170);
    v.warn_fg_color = c(161, 161, 170);
    v.error_fg_color = c(229, 72, 77);

    v.weak_text_color = Some(c(161, 161, 170));
    v.striped = true;
    ctx.set_visuals(v);

    // Two problems the default style leaves: lists end mid-row with nothing
    // to say they scroll, because a floating bar is invisible at rest, and
    // the body text is 12.5px, which is small for a table of numbers.
    let mut style = (*ctx.style()).clone();
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    // Solid alone is still invisible when the list is at rest.
    style.spacing.scroll.dormant_handle_opacity = 0.45;
    style.spacing.item_spacing = egui::vec2(6.0, 5.0);
    style.visuals.disabled_alpha = 0.6;
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.interact_size.y = 22.0;
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
    ]
    .into();
    ctx.set_style(style);
}
fn install_japanese_font(ctx: &egui::Context) -> Option<String> {
    for path in JP_FONT_CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "jp".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(family).or_default().insert(0, "jp".to_owned());
        }
        ctx.set_fonts(fonts);
        return Some((*path).to_string());
    }
    None
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Fits a 1536x864 display: 820 plus the title bar and taskbar did not.
        .with_inner_size([1180.0, 740.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("XXSE - Monster Hunter XX (NX) Save Editor"),
        ..Default::default()
    };

    eframe::run_native(
        "mhnxeditor",
        options,
        Box::new(|cc| {
            apply_theme(&cc.egui_ctx);
            let font = install_japanese_font(&cc.egui_ctx);
            let mut app = app::MhnxEditor::default();
            match font {
                Some(p) => app.status = format!("日本語フォント: {}", p),
                None => app.status = "日本語フォントが見つかりません (英数字のみ表示されます)".to_owned(),
            }
            // Headless smoke test / screenshot mode.
            if let Ok(p) = std::env::var("MHNXEDITOR_SCREENSHOT") {
                app.screenshot_path = Some(std::path::PathBuf::from(p));
            }
            if let Ok(t) = std::env::var("MHNXEDITOR_OPEN_TAB") {
                app.tab = match t.as_str() {
                    "items" => app::Tab::Items,
                    "quest" => app::Tab::Quest,
                    "dlc" => app::Tab::Dlc,
                    "charm" => app::Tab::Charm,
                    "equip" => app::Tab::Equip,
                    "hunts" => app::Tab::Hunts,
                    "guild" => app::Tab::Guild,
                    "palico" => app::Tab::Palico,
                    "sets" => app::Tab::Sets,
                "extras" => app::Tab::Extras,
            "about" => app::Tab::About,
                    _ => app::Tab::Save,
                };
            }
            if let Ok(s) = std::env::var("MHNXEDITOR_OPEN_SAVE") {
                app.open_save(std::path::PathBuf::from(s));
            }
            // Select one item box slot and prefill the name search, so the
            // picker can be checked without clicking.
            if let Ok(s) = std::env::var("MHNXEDITOR_ITEM_SELECT") {
                if let Ok(i) = s.parse::<usize>() {
                    app.item_selected = Some(i);
                }
            }
            if let Ok(s) = std::env::var("MHNXEDITOR_ITEM_FILTER") {
                app.item_pick_filter = s;
            }
            // Select a talisman by its box index, the way clicking it does.
            if let Ok(s) = std::env::var("MHNXEDITOR_CHARM_SELECT") {
                if let Ok(idx) = s.parse::<usize>() {
                    app.charm_index = idx;
                    if let Some(save) = &app.save {
                        if let Some(c) = save
                            .charms(app.slot)
                            .and_then(|l| l.into_iter().find(|c| c.index == idx))
                        {
                            app.charm_slots = c.slots;
                            app.charm_skill1 = c.skill1;
                            app.charm_points1 = c.points1;
                            app.charm_skill2 = c.skill2;
                            app.charm_points2 = c.points2;
                        }
                    }
                }
            }
            if let Ok(s) = std::env::var("MHNXEDITOR_OPEN_ARC") {
                app.open_arc(std::path::PathBuf::from(s));
            }
            // Select one entry of the open .arc (index into the entry list).
            if let Ok(s) = std::env::var("MHNXEDITOR_OPEN_ENTRY") {
                if let Ok(i) = s.parse::<usize>() {
                    app.arc_selected = i;
                    app.select_arc_entry(i);
                }
            }
            // Open one of the save's own quests, the way the DLC tab's 編集
            // button does, so the save -> quest editor loop can be driven
            // headlessly for screenshots and smoke tests.
            if let Ok(s) = std::env::var("MHNXEDITOR_OPEN_QUEST") {
                if let Ok(id) = s.parse::<i32>() {
                    app.open_arc_from_save(id);
                }
            }
            // Optionally send the edited arc straight back into the save.
            if std::env::var("MHNXEDITOR_WRITE_BACK").is_ok() {
                app.write_arc_to_save();
            }
            // ... and write the resulting save somewhere so the whole loop can
            // be verified byte for byte without a human clicking.
            if let Ok(p) = std::env::var("MHNXEDITOR_SAVE_OUT") {
                // Go through the same path the 上書き保存 button uses, so the
                // hook exercises the real write including its backup.
                app.write_current(std::path::PathBuf::from(&p));
                app.status = app.message.clone();
            }
            Ok(Box::new(app))
        }),
    )
}
