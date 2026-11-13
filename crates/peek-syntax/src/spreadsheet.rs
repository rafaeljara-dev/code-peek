use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::Path;

use anyhow::{bail, Context, Result};
use calamine::{open_workbook, Data, ExcelDateTime, Ods, Reader, Sheets, Xls, Xlsb, Xlsx};
use peek_core::TextBuffer;

const MAX_SPREADSHEET_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SHEET_ROWS: usize = 250_000;
const MAX_SHEET_COLS: usize = 1_024;
const MAX_TOTAL_CELLS: usize = 2_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Empty,
    Text,
    Number,
    Bool,
    Date,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetCell {
    pub text: String,
    pub kind: CellKind,
}

#[derive(Debug, Clone)]
pub struct SheetData {
    pub name: String,
    pub rows: Vec<Vec<SheetCell>>,
    pub max_cols: usize,
    pub truncated: bool,
    /// Workbook sheets are parsed on first use; `false` means `rows` is still empty.
    pub loaded: bool,
}

type Workbook = Sheets<BufReader<File>>;

pub struct SpreadsheetDocument {
    pub sheets: Vec<SheetData>,
    workbook: Option<Workbook>,
    loaded_cells: usize,
}

pub fn is_spreadsheet_extension(path: &Path) -> bool {
    matches!(
        lower_extension(path).as_str(),
        "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" | "csv" | "tsv"
    )
}

pub fn is_delimited_text(path: &Path) -> bool {
    matches!(lower_extension(path).as_str(), "csv" | "tsv")
}

fn lower_extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Convert a 0-based column index into Excel letters (0 -> A, 25 -> Z, 26 -> AA).
pub fn column_label(mut index: usize) -> String {
    let mut chars = Vec::new();
    loop {
        let rem = index % 26;
        chars.push((b'A' + rem as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    chars.into_iter().rev().collect()
}

/// Streams a rope without materializing a second copy of the file.
struct RopeReader<'a> {
    chunks: ropey::iter::Chunks<'a>,
    current: &'a [u8],
}

impl Read for RopeReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.current.is_empty() {
            match self.chunks.next() {
                Some(chunk) => self.current = chunk.as_bytes(),
                None => return Ok(0),
            }
        }
        let n = out.len().min(self.current.len());
        out[..n].copy_from_slice(&self.current[..n]);
        self.current = &self.current[n..];
        Ok(n)
    }
}

impl SpreadsheetDocument {
    /// Opens a binary workbook. Only the first sheet is parsed; the rest load on demand.
    pub fn from_file(path: &Path) -> Result<Self> {
        let meta =
            fs::metadata(path).with_context(|| format!("No se pudo leer {}", path.display()))?;
        if meta.len() > MAX_SPREADSHEET_FILE_BYTES {
            bail!(
                "La hoja de cálculo supera el límite seguro de {} MB",
                MAX_SPREADSHEET_FILE_BYTES / (1024 * 1024)
            );
        }
        if is_delimited_text(path) {
            return Self::from_buffer(path, &TextBuffer::from_file(path)?);
        }
        let failed = || format!("No se pudo abrir la hoja de cálculo {}", path.display());
        // calamine's own dispatcher is case-sensitive: `INFORME.XLSX` must open too.
        let workbook: Workbook = match lower_extension(path).as_str() {
            "xls" => Sheets::Xls(open_workbook::<Xls<_>, _>(path).with_context(failed)?),
            "xlsb" => Sheets::Xlsb(open_workbook::<Xlsb<_>, _>(path).with_context(failed)?),
            "ods" => Sheets::Ods(open_workbook::<Ods<_>, _>(path).with_context(failed)?),
            _ => Sheets::Xlsx(open_workbook::<Xlsx<_>, _>(path).with_context(failed)?),
        };
        let sheet_names = workbook.sheet_names();
        if sheet_names.is_empty() {
            bail!("El libro no contiene hojas");
        }
        let sheets = sheet_names
            .into_iter()
            .map(|name| SheetData {
                name,
                rows: Vec::new(),
                max_cols: 1,
                truncated: false,
                loaded: false,
            })
            .collect();
        let mut doc = Self {
            sheets,
            workbook: Some(workbook),
            loaded_cells: 0,
        };
        doc.ensure_sheet_loaded(0);
        Ok(doc)
    }

    /// Builds the table view of a CSV/TSV from the text buffer that is already in memory.
    pub fn from_buffer(path: &Path, buffer: &TextBuffer) -> Result<Self> {
        let delimiter = if lower_extension(path) == "tsv" {
            b'\t'
        } else {
            let sample: String = buffer.rope.lines().take(5).map(String::from).collect();
            detect_csv_delimiter(&sample)
        };
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Datos");
        let reader = RopeReader {
            chunks: buffer.rope.chunks(),
            current: &[],
        };
        Self::collect_csv(name, reader, delimiter)
    }

    pub fn from_csv_str(name: &str, text: &str, delimiter: u8) -> Result<Self> {
        Self::collect_csv(name, text.as_bytes(), delimiter)
    }

    fn collect_csv<R: Read>(name: &str, source: R, delimiter: u8) -> Result<Self> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(delimiter)
            .from_reader(source);
        let mut rows = Vec::new();
        let mut max_cols = 0usize;
        let mut total_cells = 0usize;
        let mut truncated = false;

        for result in reader.records() {
            if rows.len() >= MAX_SHEET_ROWS || total_cells >= MAX_TOTAL_CELLS {
                truncated = true;
                break;
            }
            let record = result.context("Error al analizar fila CSV/TSV")?;
            let col_count = record.len().min(MAX_SHEET_COLS);
            if record.len() > MAX_SHEET_COLS {
                truncated = true;
            }
            let mut row = Vec::with_capacity(col_count);
            for field in record.iter().take(col_count) {
                row.push(SheetCell {
                    text: field.to_owned(),
                    kind: classify_csv_value(field),
                });
            }
            while row.last().is_some_and(|c| c.kind == CellKind::Empty) {
                row.pop();
            }
            max_cols = max_cols.max(row.len());
            total_cells += row.len();
            rows.push(row);
        }

        Ok(Self {
            sheets: vec![SheetData {
                name: name.to_owned(),
                rows,
                max_cols: max_cols.max(1),
                truncated,
                loaded: true,
            }],
            workbook: None,
            loaded_cells: total_cells,
        })
    }

    /// Parses one workbook sheet the first time it is shown or searched.
    pub fn ensure_sheet_loaded(&mut self, index: usize) {
        let Some(sheet) = self.sheets.get_mut(index) else {
            return;
        };
        if sheet.loaded {
            return;
        }
        sheet.loaded = true;
        let Some(workbook) = self.workbook.as_mut() else {
            return;
        };
        if self.loaded_cells >= MAX_TOTAL_CELLS {
            sheet.truncated = true;
            return;
        }
        let Ok(range) = workbook.worksheet_range(&sheet.name) else {
            return;
        };
        // The range begins at the first used cell; pad so row numbers and column letters match Excel.
        let (first_row, first_col) = range
            .start()
            .map_or((0, 0), |(r, c)| (r as usize, c as usize));
        let mut rows: Vec<Vec<SheetCell>> = Vec::new();
        let mut max_cols = 0usize;
        let mut truncated = first_row >= MAX_SHEET_ROWS || first_col >= MAX_SHEET_COLS;
        if !truncated {
            rows.resize_with(first_row, Vec::new);
            for raw_row in range.rows() {
                if rows.len() >= MAX_SHEET_ROWS || self.loaded_cells >= MAX_TOTAL_CELLS {
                    truncated = true;
                    break;
                }
                let limit = raw_row.len().min(MAX_SHEET_COLS - first_col);
                if raw_row.len() > limit {
                    truncated = true;
                }
                let mut cells = Vec::with_capacity(first_col + limit);
                cells.resize_with(first_col, empty_cell);
                cells.extend(raw_row.iter().take(limit).map(format_calamine_cell));
                while cells.last().is_some_and(|c| c.kind == CellKind::Empty) {
                    cells.pop();
                }
                max_cols = max_cols.max(cells.len());
                self.loaded_cells += cells.len();
                rows.push(cells);
            }
            while rows.last().is_some_and(Vec::is_empty) {
                rows.pop();
            }
        }
        sheet.rows = rows;
        sheet.max_cols = max_cols.max(1);
        sheet.truncated = truncated;
    }

    pub fn ensure_all_loaded(&mut self) {
        for index in 0..self.sheets.len() {
            self.ensure_sheet_loaded(index);
        }
    }
}

fn empty_cell() -> SheetCell {
    SheetCell {
        text: String::new(),
        kind: CellKind::Empty,
    }
}

fn detect_csv_delimiter(sample: &str) -> u8 {
    let first_lines: String = sample.lines().take(5).collect::<Vec<_>>().join("\n");
    let commas = first_lines.matches(',').count();
    let semicolons = first_lines.matches(';').count();
    let tabs = first_lines.matches('\t').count();
    if tabs > commas && tabs >= semicolons {
        b'\t'
    } else if semicolons > commas {
        b';'
    } else {
        b','
    }
}

fn classify_csv_value(field: &str) -> CellKind {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        CellKind::Empty
    } else if trimmed.eq_ignore_ascii_case("true") || trimmed.eq_ignore_ascii_case("false") {
        CellKind::Bool
    } else if trimmed.parse::<f64>().is_ok() {
        CellKind::Number
    } else {
        CellKind::Text
    }
}

fn format_calamine_cell(value: &Data) -> SheetCell {
    let (text, kind) = match value {
        Data::Empty => (String::new(), CellKind::Empty),
        Data::String(s) => (
            s.clone(),
            if s.trim().is_empty() {
                CellKind::Empty
            } else {
                CellKind::Text
            },
        ),
        Data::Int(i) => (i.to_string(), CellKind::Number),
        Data::Float(f) => (format_float(*f), CellKind::Number),
        Data::Bool(b) => (
            if *b { "VERDADERO" } else { "FALSO" }.to_owned(),
            CellKind::Bool,
        ),
        Data::DateTime(dt) => (format_excel_datetime(dt), CellKind::Date),
        Data::DateTimeIso(s) | Data::DurationIso(s) => (s.replace('T', " "), CellKind::Date),
        Data::Error(err) => (format!("#{err}"), CellKind::Error),
    };
    SheetCell { text, kind }
}

/// Excel stores dates as serial numbers; show them as `AAAA-MM-DD [HH:MM[:SS]]`.
fn format_excel_datetime(value: &ExcelDateTime) -> String {
    let serial = value.as_f64();
    if !serial.is_finite() {
        return serial.to_string();
    }
    if value.is_duration() {
        let total_seconds = (serial * 86_400.0).round() as i64;
        let sign = if total_seconds < 0 { "-" } else { "" };
        let total_seconds = total_seconds.abs();
        return format!(
            "{sign}{}:{:02}:{:02}",
            total_seconds / 3600,
            total_seconds / 60 % 60,
            total_seconds % 60
        );
    }
    let (year, month, day, hour, minute, second, _) = value.to_ymd_hms_milli();
    let time = if second == 0 {
        format!("{hour:02}:{minute:02}")
    } else {
        format!("{hour:02}:{minute:02}:{second:02}")
    };
    if (0.0..1.0).contains(&serial) {
        time
    } else if hour == 0 && minute == 0 && second == 0 {
        format!("{year:04}-{month:02}-{day:02}")
    } else {
        format!("{year:04}-{month:02}-{day:02} {time}")
    }
}

fn format_float(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    if (v - v.round()).abs() < 1e-9 && v.abs() < 1e15 {
        return format!("{}", v.round() as i64);
    }
    let formatted = format!("{v:.6}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::ExcelDateTimeType;
    use std::io::Write;

    #[test]
    fn generates_excel_column_labels() {
        assert_eq!(column_label(0), "A");
        assert_eq!(column_label(25), "Z");
        assert_eq!(column_label(26), "AA");
        assert_eq!(column_label(27), "AB");
    }

    #[test]
    fn parses_csv_with_semicolon_and_unicode() {
        let doc = SpreadsheetDocument::from_csv_str(
            "ventas.csv",
            "Producto;Precio;Activo\nCafé niño;42.50;true\nTé;;false\n",
            detect_csv_delimiter("Producto;Precio;Activo\nCafé niño;42.50;true\n"),
        )
        .unwrap();
        assert_eq!(doc.sheets.len(), 1);
        assert_eq!(doc.sheets[0].max_cols, 3);
        assert_eq!(doc.sheets[0].rows[1][0].text, "Café niño");
        assert_eq!(doc.sheets[0].rows[1][1].kind, CellKind::Number);
        assert_eq!(doc.sheets[0].rows[1][2].kind, CellKind::Bool);
    }

    #[test]
    fn detects_spreadsheet_extensions_in_any_case() {
        assert!(is_spreadsheet_extension(Path::new("informe.xlsx")));
        assert!(is_spreadsheet_extension(Path::new("INFORME.XLSX")));
        assert!(is_spreadsheet_extension(Path::new("datos.CSV")));
        assert!(is_spreadsheet_extension(Path::new("libro.ods")));
        assert!(!is_spreadsheet_extension(Path::new("README.md")));
        assert!(is_delimited_text(Path::new("tabla.tsv")));
        assert!(!is_delimited_text(Path::new("libro.xlsx")));
    }

    #[test]
    fn formats_excel_dates_times_and_durations_instead_of_serial_numbers() {
        let date = |v| ExcelDateTime::new(v, ExcelDateTimeType::DateTime, false);
        assert_eq!(format_excel_datetime(&date(45943.0)), "2025-10-13");
        assert_eq!(format_excel_datetime(&date(45943.5)), "2025-10-13 12:00");
        assert_eq!(format_excel_datetime(&date(0.75)), "18:00");
        let duration = ExcelDateTime::new(1.5, ExcelDateTimeType::TimeDelta, false);
        assert_eq!(format_excel_datetime(&duration), "36:00:00");
    }

    #[test]
    fn legacy_windows_csv_keeps_accents_in_every_encoding() {
        let dir = tempfile::tempdir().unwrap();
        let expected = "Año;Niño;Canción\n2024;Peña;Sí\n";
        let (latin, _, _) = encoding_rs::WINDOWS_1252.encode(expected);
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(expected.encode_utf16().flat_map(u16::to_le_bytes));
        let mut utf8_bom = vec![0xEF, 0xBB, 0xBF];
        utf8_bom.extend_from_slice(expected.as_bytes());
        for (name, bytes) in [
            ("latin.csv", latin.to_vec()),
            ("utf16.csv", utf16),
            ("bom.csv", utf8_bom),
            ("utf8.CSV", expected.as_bytes().to_vec()),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            let doc = SpreadsheetDocument::from_file(&path).unwrap();
            let rows = &doc.sheets[0].rows;
            assert_eq!(rows[0][0].text, "Año", "{name}");
            assert_eq!(rows[0][2].text, "Canción", "{name}");
            assert_eq!(rows[1][1].text, "Peña", "{name}");
            assert_eq!(rows[1][2].text, "Sí", "{name}");
        }
    }

    fn write_xlsx(path: &Path) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, body: &str| {
            zip.start_file(name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };
        add(
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>"#,
        );
        add(
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        );
        add(
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Año 2025" sheetId="1" r:id="rId1"/><sheet name="Niños" sheetId="2" r:id="rId2"/></sheets></workbook>"#,
        );
        add(
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#,
        );
        add(
            "xl/styles.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14" applyNumberFormat="1"/></cellXfs></styleSheet>"#,
        );
        add(
            "xl/sharedStrings.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2"><si><t>Canción</t></si><si><t>Peña</t></si></sst>"#,
        );
        add(
            "xl/worksheets/sheet1.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="2"><c r="B2" t="s"><v>0</v></c><c r="C2" s="1"><v>45943</v></c><c r="D2"><v>12.5</v></c></row></sheetData></worksheet>"#,
        );
        add(
            "xl/worksheets/sheet2.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>1</v></c></row></sheetData></worksheet>"#,
        );
        zip.finish().unwrap();
    }

    #[test]
    fn xlsx_keeps_accents_dates_cell_positions_and_loads_sheets_lazily() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("INFORME.XLSX");
        write_xlsx(&path);
        let mut doc = SpreadsheetDocument::from_file(&path).unwrap();
        assert_eq!(doc.sheets[0].name, "Año 2025");
        assert_eq!(doc.sheets[1].name, "Niños");
        assert!(doc.sheets[0].loaded && !doc.sheets[1].loaded);
        // Data starts at B2: the padding keeps Excel's row numbers and column letters.
        let first = &doc.sheets[0];
        assert!(first.rows[0].is_empty());
        assert_eq!(first.rows[1][0].kind, CellKind::Empty);
        assert_eq!(first.rows[1][1].text, "Canción");
        assert_eq!(first.rows[1][2].text, "2025-10-13");
        assert_eq!(first.rows[1][2].kind, CellKind::Date);
        assert_eq!(first.rows[1][3].text, "12.5");
        doc.ensure_all_loaded();
        assert_eq!(doc.sheets[1].rows[0][0].text, "Peña");
    }
}
