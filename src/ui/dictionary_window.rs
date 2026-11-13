//! The dictionary window: a second, ordinary window opened by right-clicking
//! the capsule. Like the capsule it is a Humble Object — the replacement
//! logic lives in `crate::dictionary`; this file only edits the entry list.
//!
//! Three parts, top to bottom: fix the last result (select the wrong part,
//! type what it should have been), the full entry table, and a field to try
//! the dictionary without recording.

use std::sync::Arc;

use eframe::egui::{self, Color32, RichText, TextEdit};

use crate::dictionary::{Dictionary, DictionaryStore, Entry};

const WINDOW_SIZE: [f32; 2] = [560.0, 600.0];
const MIN_WINDOW_SIZE: [f32; 2] = [440.0, 380.0];
/// Width of the two text columns of the entry table.
const ENTRY_WIDTH: f32 = 200.0;

pub struct DictionaryWindow {
    store: Arc<DictionaryStore>,
    open: bool,
    fonts_installed: bool,
    /// The table as shown, including rows still being typed.
    rows: Vec<Entry>,
    /// The "add" form.
    from: String,
    to: String,
    /// The selection last copied into `from`, so typing over it sticks.
    last_selection: String,
    test_input: String,
    save_error: Option<String>,
}

impl DictionaryWindow {
    pub fn new(store: Arc<DictionaryStore>) -> Self {
        Self {
            store,
            open: false,
            fonts_installed: false,
            rows: Vec::new(),
            from: String::new(),
            to: String::new(),
            last_selection: String::new(),
            test_input: String::new(),
            save_error: None,
        }
    }

    /// Opens the window, or raises it when it is already open.
    pub fn open(&mut self, ctx: &egui::Context) {
        if !self.fonts_installed {
            // The capsule draws no text, so the (large) Japanese font is
            // only loaded once this window is actually wanted.
            install_japanese_font(ctx);
            self.fonts_installed = true;
        }
        if self.open {
            ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Focus);
            return;
        }
        self.rows = self.store.entries();
        self.save_error = None;
        self.open = true;
    }

    /// Draws the window if it is open. Call once per frame from the root.
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let builder = egui::ViewportBuilder::default()
            .with_title("voice_input — 辞書")
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(MIN_WINDOW_SIZE);
        ctx.show_viewport_immediate(viewport_id(), builder, |ctx, _class| {
            egui::CentralPanel::default().show(ctx, |ui| self.contents(ui));
            if ctx.input(|i| i.viewport().close_requested()) {
                self.open = false;
            }
        });
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        let before = self.rows.clone();

        self.correction_section(ui);
        ui.add_space(8.0);
        ui.separator();
        self.test_and_footer(ui);
        ui.separator();
        self.table_section(ui);

        if self.rows != before {
            self.save();
        }
    }

    /// "Fix the last result": the quickest way to add an entry.
    fn correction_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("直前の認識結果");
        match self.store.last_text() {
            Some(text) => {
                // A `&str` buffer is read-only but still selectable.
                let mut shown = text.as_str();
                let output = TextEdit::multiline(&mut shown)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .show(ui);
                let selection = output
                    .cursor_range
                    .map(|range| range.slice_str(&text).trim().to_owned())
                    .unwrap_or_default();
                if !selection.is_empty() && selection != self.last_selection {
                    self.from.clone_from(&selection);
                }
                if output.cursor_range.is_some() {
                    self.last_selection = selection;
                }
                ui.weak("間違っている部分をドラッグで選ぶと「聞こえ方」に入ります。");
            }
            None => {
                ui.weak("まだ認識結果がありません。下の欄に直接入力して登録できます。");
            }
        }

        ui.add_space(4.0);
        let mut submit = false;
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.from)
                    .hint_text("聞こえ方 (例: クラウドコード)")
                    .desired_width(190.0),
            );
            ui.label("→");
            let to = ui.add(
                TextEdit::singleline(&mut self.to)
                    .hint_text("表記 (例: Claude Code)")
                    .desired_width(170.0),
            );
            let ready = !self.from.trim().is_empty() && !self.to.trim().is_empty();
            let enter = to.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let clicked = ui
                .add_enabled(ready, egui::Button::new("辞書に登録"))
                .clicked();
            submit = ready && (clicked || enter);
        });
        if submit {
            self.register();
        }
    }

    /// Adds the form's entry; an existing `from` gets its `to` updated.
    fn register(&mut self) {
        let from = self.from.trim().to_owned();
        let to = self.to.trim().to_owned();
        match self.rows.iter_mut().find(|row| row.from.trim() == from) {
            Some(row) => row.to = to,
            None => self.rows.push(Entry::new(from, to)),
        }
        self.from.clear();
        self.to.clear();
    }

    fn table_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(format!("辞書 ({}件)", self.rows.len()));
            if ui.button("行を追加").clicked() {
                self.rows.push(Entry::default());
            }
        });
        ui.weak("1つの表記に、聞こえ方を何行でも登録できます。");
        ui.add_space(4.0);

        let mut remove = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("dictionary_entries")
                    .num_columns(3)
                    .min_col_width(ENTRY_WIDTH)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("聞こえ方");
                        ui.strong("表記");
                        ui.end_row();
                        for (index, row) in self.rows.iter_mut().enumerate() {
                            ui.add(TextEdit::singleline(&mut row.from).desired_width(ENTRY_WIDTH));
                            ui.add(TextEdit::singleline(&mut row.to).desired_width(ENTRY_WIDTH));
                            if ui.button("削除").clicked() {
                                remove = Some(index);
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(index) = remove {
            self.rows.remove(index);
        }
    }

    /// Try the dictionary on typed text, plus where it is saved.
    fn test_and_footer(&mut self, ui: &mut egui::Ui) {
        ui.heading("テスト");
        ui.add(
            TextEdit::singleline(&mut self.test_input)
                .hint_text("ここに入力すると、置換後の結果を確認できます")
                .desired_width(f32::INFINITY),
        );
        if !self.test_input.is_empty() {
            let result = Dictionary::new(self.rows.clone()).apply(&self.test_input);
            ui.horizontal_wrapped(|ui| {
                ui.weak("結果:");
                ui.label(result);
            });
        }
        ui.add_space(4.0);
        if let Some(error) = &self.save_error {
            ui.colored_label(Color32::from_rgb(255, 159, 10), error);
        } else if let Some(path) = self.store.path() {
            ui.label(
                RichText::new(format!("保存先: {}", path.display()))
                    .weak()
                    .small(),
            );
        }
    }

    /// Rows still missing a side stay on screen but are not saved.
    fn save(&mut self) {
        let complete = self
            .rows
            .iter()
            .filter(|row| !row.from.trim().is_empty() && !row.to.trim().is_empty())
            .map(|row| Entry::new(row.from.trim(), row.to.trim()))
            .collect();
        self.save_error = match self.store.set_entries(complete) {
            Ok(()) => None,
            Err(e) => {
                log::error!("{e}");
                Some(format!("保存できませんでした: {e}"))
            }
        };
    }
}

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("voice_input.dictionary")
}

/// Where a font with Japanese glyphs usually is. egui ships none.
#[cfg(target_os = "windows")]
const JAPANESE_FONTS: &[&str] = &[
    r"C:\Windows\Fonts\YuGothM.ttc",
    r"C:\Windows\Fonts\meiryo.ttc",
    r"C:\Windows\Fonts\msgothic.ttc",
];
#[cfg(target_os = "macos")]
const JAPANESE_FONTS: &[&str] = &[
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/Library/Fonts/Arial Unicode.ttf",
];
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const JAPANESE_FONTS: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
    "/usr/share/fonts/truetype/takao-gothic/TakaoGothic.ttf",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
];

/// Adds the first system Japanese font found as a fallback for every font
/// family. Without one the window still works but Japanese shows as boxes.
fn install_japanese_font(ctx: &egui::Context) {
    let Some((path, bytes)) = JAPANESE_FONTS
        .iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (path, bytes)))
    else {
        log::warn!("no Japanese font found; the dictionary window cannot show Japanese text");
        return;
    };
    log::debug!("dictionary window font: {path}");
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "japanese".to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in fonts.families.values_mut() {
        family.push("japanese".to_owned());
    }
    ctx.set_fonts(fonts);
}
