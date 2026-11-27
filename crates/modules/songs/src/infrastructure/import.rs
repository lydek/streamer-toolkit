//! Reading an imported song list: an Excel workbook (every sheet) or a CSV file (UTF-8, with or without a BOM).

use std::io::Cursor;

use calamine::{Reader, Xlsx};

use crate::domain::Sheet;

/// The file's size is capped at the route; this caps the rows read per sheet.
const ROWS_MAX: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Xlsx,
    Csv,
}

/// The sheets of the file; `None` when it cannot be read as `format`.
pub(crate) fn read(bytes: &[u8], format: Format) -> Option<Vec<Sheet>> {
    match format {
        Format::Csv => read_csv(bytes).map(|rows| vec![Sheet { name: None, rows }]),
        Format::Xlsx => read_xlsx(bytes),
    }
}

fn read_csv(bytes: &[u8]) -> Option<Vec<Vec<String>>> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(bytes).ok()?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes);
    let mut rows = Vec::new();
    for record in reader.records().take(ROWS_MAX) {
        rows.push(record.ok()?.iter().map(str::to_owned).collect());
    }
    Some(rows)
}

fn read_xlsx(bytes: &[u8]) -> Option<Vec<Sheet>> {
    let mut book: Xlsx<_> = Xlsx::new(Cursor::new(bytes)).ok()?;
    let mut sheets = Vec::new();
    for name in book.sheet_names() {
        let Ok(range) = book.worksheet_range(&name) else {
            continue;
        };
        let rows = range
            .rows()
            .take(ROWS_MAX)
            .map(|row| row.iter().map(|cell| cell.to_string()).collect())
            .collect();
        sheets.push(Sheet {
            name: Some(name),
            rows,
        });
    }
    Some(sheets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_with_a_bom_and_quotes_reads_as_rows() {
        let file = "\u{FEFF}歌名,歌手\n\"晴天\",周杰倫\n\"Hello, World\",\n".as_bytes();
        let sheets = read(file, Format::Csv).unwrap();
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].name, None);
        assert_eq!(sheets[0].rows[0], ["歌名", "歌手"]);
        assert_eq!(sheets[0].rows[2], ["Hello, World", ""]);
    }

    #[test]
    fn a_file_that_is_not_what_it_says_is_refused() {
        assert!(read(b"\xFF\xFE\x00broken", Format::Csv).is_none());
        assert!(read(b"not a zip", Format::Xlsx).is_none());
    }
}
