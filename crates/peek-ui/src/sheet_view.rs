use peek_syntax::{column_label, CellKind, SheetData, SpreadsheetDocument};
use slint::{ModelRc, SharedString, VecModel};

use crate::doc_view::text_em;
use crate::{SheetCellItem, SheetHeaderItem, SheetRowItem};

pub const SHEET_ROW_WINDOW: usize = 100;
pub const SHEET_COL_WINDOW: usize = 16;
const MIN_COL_EM: f32 = 5.0;
const MAX_COL_EM: f32 = 30.0;

#[derive(Default)]
pub struct SheetViewState {
    pub document: Option<SpreadsheetDocument>,
    pub active_sheet: usize,
    pub row_start: usize,
    pub col_start: usize,
    pub selected_cell: Option<(usize, usize)>,
    pub find_query: String,
    pub find_matches: Vec<(usize, usize, usize)>,
    pub find_cursor: usize,
    /// The whole window must be rebuilt.
    pub dirty: bool,
    /// Absolute rows whose selection changed; they are patched in place.
    pub dirty_rows: Vec<usize>,
}

pub struct PreparedSheetHeader {
    pub label: String,
    pub col_index: i32,
    /// Column width in tenths of an em of the sheet font.
    pub width: i32,
}

pub struct PreparedSheetCell {
    pub col_index: i32,
    pub text: String,
    pub kind: i32,
    pub selected: bool,
    pub matched: bool,
}

pub struct PreparedSheetRow {
    pub row_number: i32,
    pub cells: Vec<PreparedSheetCell>,
}

impl PreparedSheetRow {
    pub fn into_slint(self) -> SheetRowItem {
        SheetRowItem {
            row_number: self.row_number,
            cells: ModelRc::new(VecModel::from(
                self.cells
                    .into_iter()
                    .map(|c| SheetCellItem {
                        col_index: c.col_index,
                        text: c.text.into(),
                        kind: c.kind,
                        selected: c.selected,
                        matched: c.matched,
                    })
                    .collect::<Vec<_>>(),
            )),
        }
    }
}

#[derive(Default)]
pub struct PreparedSheetModels {
    pub sheet_names: Vec<String>,
    pub active_sheet: i32,
    pub headers: Vec<PreparedSheetHeader>,
    pub rows: Vec<PreparedSheetRow>,
    pub total_rows: i32,
    pub total_cols: i32,
    pub row_start: i32,
    pub col_start: i32,
    pub has_prev_rows: bool,
    pub has_next_rows: bool,
    pub has_prev_cols: bool,
    pub has_next_cols: bool,
    pub truncated: bool,
}

impl PreparedSheetModels {
    pub fn into_slint(self) -> (Vec<SharedString>, Vec<SheetHeaderItem>, Vec<SheetRowItem>) {
        let tabs = self
            .sheet_names
            .into_iter()
            .map(SharedString::from)
            .collect();
        let headers = self
            .headers
            .into_iter()
            .map(|h| SheetHeaderItem {
                label: h.label.into(),
                col_index: h.col_index,
                width: h.width,
            })
            .collect();
        let rows = self
            .rows
            .into_iter()
            .map(PreparedSheetRow::into_slint)
            .collect();
        (tabs, headers, rows)
    }
}

impl SheetViewState {
    fn reset_view(&mut self) {
        self.row_start = 0;
        self.col_start = 0;
        self.selected_cell = None;
        self.dirty_rows.clear();
        self.dirty = true;
    }

    fn reset_find(&mut self) {
        self.find_query.clear();
        self.find_matches.clear();
        self.find_cursor = 0;
    }

    pub fn clear(&mut self) {
        self.document = None;
        self.active_sheet = 0;
        self.reset_find();
        self.reset_view();
    }

    pub fn set_document(&mut self, doc: SpreadsheetDocument) {
        self.document = Some(doc);
        self.active_sheet = 0;
        self.reset_find();
        self.reset_view();
    }

    fn sheet(&self) -> Option<&SheetData> {
        self.document.as_ref()?.sheets.get(self.active_sheet)
    }

    pub fn select_sheet(&mut self, index: usize) {
        let Some(doc) = &mut self.document else {
            return;
        };
        if index < doc.sheets.len() && index != self.active_sheet {
            doc.ensure_sheet_loaded(index);
            self.active_sheet = index;
            self.reset_view();
        }
    }

    /// Restores a remembered position (tab switch or reload), clamped to the sheet.
    pub fn restore(&mut self, sheet: usize, row_start: usize, col_start: usize) {
        self.select_sheet(sheet);
        if let Some((rows, cols)) = self.sheet().map(|data| (data.rows.len(), data.max_cols)) {
            self.row_start = row_start.min(rows.saturating_sub(SHEET_ROW_WINDOW));
            self.col_start = col_start.min(cols.saturating_sub(SHEET_COL_WINDOW));
            self.dirty = true;
        }
    }

    pub fn select_cell(&mut self, row: usize, col: usize) {
        if let Some((previous, _)) = self.selected_cell {
            self.dirty_rows.push(previous);
        }
        self.dirty_rows.push(row);
        self.selected_cell = Some((row, col));
    }

    pub fn selected_text(&self) -> String {
        let Some((r, c)) = self.selected_cell else {
            return String::new();
        };
        self.sheet()
            .and_then(|sheet| sheet.rows.get(r))
            .and_then(|row| row.get(c))
            .map(|cell| cell.text.clone())
            .unwrap_or_default()
    }

    /// Excel-style reference of the selected cell, e.g. `B12`.
    pub fn selected_ref(&self) -> String {
        self.selected_cell
            .map(|(r, c)| format!("{}{}", column_label(c), r + 1))
            .unwrap_or_default()
    }

    pub fn selected_row_tsv(&self) -> String {
        let row_idx = self.selected_cell.map_or(self.row_start, |(r, _)| r);
        let Some(row) = self.sheet().and_then(|sheet| sheet.rows.get(row_idx)) else {
            return String::new();
        };
        row.iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\t")
    }

    pub fn step_rows(&mut self, forward: bool) {
        let Some(sheet) = self.sheet() else {
            return;
        };
        let max_start = sheet.rows.len().saturating_sub(SHEET_ROW_WINDOW);
        let step = SHEET_ROW_WINDOW / 2;
        let next = if forward {
            (self.row_start + step).min(max_start)
        } else {
            self.row_start.saturating_sub(step)
        };
        if next != self.row_start {
            self.row_start = next;
            self.dirty = true;
        }
    }

    pub fn step_cols(&mut self, forward: bool) {
        let Some(sheet) = self.sheet() else {
            return;
        };
        let max_start = sheet.max_cols.saturating_sub(SHEET_COL_WINDOW);
        let step = SHEET_COL_WINDOW / 2;
        let next = if forward {
            (self.col_start + step).min(max_start)
        } else {
            self.col_start.saturating_sub(step)
        };
        if next != self.col_start {
            self.col_start = next;
            self.dirty = true;
        }
    }

    pub fn find(&mut self, query: &str, forward: bool, case_sensitive: bool) -> String {
        let Some(doc) = &mut self.document else {
            return "Sin hoja".into();
        };
        self.dirty = true;
        if query.is_empty() {
            self.reset_find();
            return String::new();
        }
        let needle = if case_sensitive {
            query.to_owned()
        } else {
            query.to_lowercase()
        };
        if self.find_query != query || self.find_matches.is_empty() {
            // Searching is the one action that needs every sheet parsed.
            doc.ensure_all_loaded();
            self.find_query = query.to_owned();
            self.find_matches.clear();
            for (s_idx, sheet) in doc.sheets.iter().enumerate() {
                for (r_idx, row) in sheet.rows.iter().enumerate() {
                    for (c_idx, cell) in row.iter().enumerate() {
                        let matched = if case_sensitive {
                            cell.text.contains(&needle)
                        } else {
                            cell.text.to_lowercase().contains(&needle)
                        };
                        if matched {
                            self.find_matches.push((s_idx, r_idx, c_idx));
                        }
                    }
                }
            }
            self.find_cursor = 0;
        } else {
            self.find_cursor = if forward {
                (self.find_cursor + 1) % self.find_matches.len()
            } else if self.find_cursor == 0 {
                self.find_matches.len() - 1
            } else {
                self.find_cursor - 1
            };
        }

        let Some(&(s_idx, r_idx, c_idx)) = self.find_matches.get(self.find_cursor) else {
            return "Sin coincidencias".into();
        };
        if let Some(sheet) = doc.sheets.get(s_idx) {
            self.active_sheet = s_idx;
            let max_r = sheet.rows.len().saturating_sub(SHEET_ROW_WINDOW);
            let max_c = sheet.max_cols.saturating_sub(SHEET_COL_WINDOW);
            // Keep the window still when the match is already visible.
            if !(self.row_start..self.row_start + SHEET_ROW_WINDOW).contains(&r_idx) {
                self.row_start = r_idx.saturating_sub(8).min(max_r);
            }
            if !(self.col_start..self.col_start + SHEET_COL_WINDOW).contains(&c_idx) {
                self.col_start = c_idx.saturating_sub(2).min(max_c);
            }
            self.selected_cell = Some((r_idx, c_idx));
        }
        format!("{} de {}", self.find_cursor + 1, self.find_matches.len())
    }

    fn col_range(&self, sheet: &SheetData) -> std::ops::Range<usize> {
        self.col_start..(self.col_start + SHEET_COL_WINDOW).min(sheet.max_cols)
    }

    fn prepare_row(&self, sheet: &SheetData, abs_r: usize) -> Option<PreparedSheetRow> {
        let row = sheet.rows.get(abs_r)?;
        let active_match = self.find_matches.get(self.find_cursor).copied();
        let cells = self
            .col_range(sheet)
            .map(|abs_c| {
                let cell = row.get(abs_c);
                PreparedSheetCell {
                    col_index: abs_c as i32,
                    text: cell.map(|c| c.text.clone()).unwrap_or_default(),
                    kind: match cell.map_or(CellKind::Empty, |c| c.kind) {
                        CellKind::Empty => 0,
                        CellKind::Text => 1,
                        CellKind::Number => 2,
                        CellKind::Bool => 3,
                        CellKind::Date => 4,
                        CellKind::Error => 5,
                    },
                    selected: self.selected_cell == Some((abs_r, abs_c)),
                    matched: active_match == Some((self.active_sheet, abs_r, abs_c)),
                }
            })
            .collect();
        Some(PreparedSheetRow {
            row_number: abs_r as i32 + 1,
            cells,
        })
    }

    /// Rows whose selection changed, as `(offset in the window, row)`.
    pub fn take_row_updates(&mut self) -> Vec<(usize, PreparedSheetRow)> {
        let mut rows = std::mem::take(&mut self.dirty_rows);
        rows.sort_unstable();
        rows.dedup();
        let Some(sheet) = self.sheet() else {
            return Vec::new();
        };
        rows.into_iter()
            .filter(|row| (self.row_start..self.row_start + SHEET_ROW_WINDOW).contains(row))
            .filter_map(|row| Some((row - self.row_start, self.prepare_row(sheet, row)?)))
            .collect()
    }

    pub fn prepare_models(&self) -> PreparedSheetModels {
        let Some(doc) = &self.document else {
            return PreparedSheetModels::default();
        };
        let sheet_names = doc.sheets.iter().map(|s| s.name.clone()).collect();
        let Some(sheet) = self.sheet() else {
            return PreparedSheetModels {
                sheet_names,
                ..Default::default()
            };
        };
        let total_rows = sheet.rows.len();
        let total_cols = sheet.max_cols;
        let row_end = (self.row_start + SHEET_ROW_WINDOW).min(total_rows);
        let cols = self.col_range(sheet);

        // Size each column to the widest value in the visible window.
        let headers = cols
            .clone()
            .map(|c| {
                let widest = sheet.rows[self.row_start..row_end]
                    .iter()
                    .filter_map(|row| row.get(c))
                    .map(|cell| text_em(&cell.text))
                    .fold(0.0f32, f32::max);
                PreparedSheetHeader {
                    label: column_label(c),
                    col_index: c as i32,
                    width: ((widest + 1.4).clamp(MIN_COL_EM, MAX_COL_EM) * 10.0).ceil() as i32,
                }
            })
            .collect();
        let rows = (self.row_start..row_end)
            .filter_map(|abs_r| self.prepare_row(sheet, abs_r))
            .collect();

        PreparedSheetModels {
            sheet_names,
            active_sheet: self.active_sheet as i32,
            headers,
            rows,
            total_rows: total_rows as i32,
            total_cols: total_cols as i32,
            row_start: self.row_start as i32,
            col_start: self.col_start as i32,
            has_prev_rows: self.row_start > 0,
            has_next_rows: row_end < total_rows,
            has_prev_cols: self.col_start > 0,
            has_next_cols: cols.end < total_cols,
            truncated: sheet.truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_cells_and_searches_across_csv_sheets() {
        let doc = SpreadsheetDocument::from_csv_str(
            "datos.csv",
            "Nombre,Valor\nAlfa,100\nBeta,250\n",
            b',',
        )
        .unwrap();
        let mut state = SheetViewState::default();
        state.set_document(doc);
        assert_eq!(state.find("Beta", true, false), "1 de 1");
        assert_eq!(state.selected_cell, Some((2, 0)));
        assert_eq!(state.selected_text(), "Beta");
        assert_eq!(state.selected_ref(), "A3");
        assert_eq!(state.selected_row_tsv(), "Beta\t250");
        let models = state.prepare_models();
        assert_eq!(models.rows.len(), 3);
        assert_eq!(models.headers.len(), 2);
        assert_eq!(models.headers[0].label, "A");
        assert!(models.rows[2].cells[0].selected);
    }

    #[test]
    fn columns_fit_their_content_and_selection_patches_only_two_rows() {
        let doc = SpreadsheetDocument::from_csv_str(
            "datos.csv",
            "Id,Descripción\n1,Una descripción bastante larga para la columna\n2,Corta\n",
            b',',
        )
        .unwrap();
        let mut state = SheetViewState::default();
        state.set_document(doc);
        let models = state.prepare_models();
        assert!(models.headers[1].width > models.headers[0].width * 3);
        assert_eq!(models.headers[0].width, 50);

        state.dirty = false;
        state.select_cell(1, 1);
        state.select_cell(2, 0);
        assert!(!state.dirty, "a click must not rebuild the whole window");
        let updates = state.take_row_updates();
        assert_eq!(
            updates
                .iter()
                .map(|(offset, _)| *offset)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert!(!updates[0].1.cells[1].selected);
        assert!(updates[1].1.cells[0].selected);
        assert!(state.take_row_updates().is_empty());
    }
}
