//! The API stats tab: `Recorder::report()` as it is, with the calls table sortable. No analysis of its own.

use std::cmp::Ordering;

use serde_json::Value;

/// The calls table's columns: the report field and its heading.
pub const CALL_COLUMNS: [(&str, &str); 8] = [
    ("key", "呼叫"),
    ("total", "次數"),
    ("per_minute", "每分鐘"),
    ("peak_10s", "10 秒最多"),
    ("avg_gap_ms", "平均間隔"),
    ("min_gap_ms", "最短間隔"),
    ("avg_handled_ms", "處理 ms"),
    ("max_in_flight", "同時"),
];

/// The calls table's order: a column of `CALL_COLUMNS` and whether it runs high to low.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: usize,
    pub descending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self {
            column: 1,
            descending: true,
        }
    }
}

impl Sort {
    /// After clicking `column`'s heading: the same column turns around; another starts high to low (names A to Z).
    pub fn clicked(self, column: usize) -> Self {
        if column == self.column {
            Self {
                descending: !self.descending,
                ..self
            }
        } else {
            Self {
                column,
                descending: column != 0,
            }
        }
    }

    /// The heading with an arrow on the sorted column.
    pub fn heading(self, column: usize) -> String {
        let label = CALL_COLUMNS[column].1;
        match (column == self.column, self.descending) {
            (false, _) => label.to_owned(),
            (true, true) => format!("{label} ▼"),
            (true, false) => format!("{label} ▲"),
        }
    }
}

/// The report's call rows in `sort` order (ties by key); an empty value sorts lowest.
pub fn sorted_calls(report: &Value, sort: Sort) -> Vec<Value> {
    let mut rows = report["calls"].as_array().cloned().unwrap_or_default();
    let field = CALL_COLUMNS[sort.column].0;
    let by_key = |a: &Value, b: &Value| a["key"].as_str().cmp(&b["key"].as_str());
    rows.sort_by(|a, b| {
        let order = match (a[field].as_f64(), b[field].as_f64()) {
            (None, None) if field == "key" => by_key(a, b),
            (x, y) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        };
        let order = if sort.descending {
            order.reverse()
        } else {
            order
        };
        order.then_with(|| by_key(a, b))
    });
    rows
}

/// A report value as table text: empty for null, numbers as JSON writes them.
pub fn cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(cell).collect::<Vec<_>>().join(" "),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report() -> Value {
        json!({"calls": [
            {"key": "GET /helix/users", "total": 3, "avg_gap_ms": 20.5},
            {"key": "GetVersion", "total": 9, "avg_gap_ms": null},
            {"key": "GET /helix/streams", "total": 3, "avg_gap_ms": 4.0},
        ]})
    }

    fn keys(rows: &[Value]) -> Vec<&str> {
        rows.iter().map(|r| r["key"].as_str().unwrap()).collect()
    }

    #[test]
    fn rows_are_the_report_in_the_picked_order() {
        let r = report();
        let rows = sorted_calls(&r, Sort::default());
        assert_eq!(
            keys(&rows),
            ["GetVersion", "GET /helix/streams", "GET /helix/users"]
        );
        assert_eq!(rows[0], r["calls"][1], "rows are the report's own values");
        let by_gap = Sort::default().clicked(4);
        assert_eq!(
            keys(&sorted_calls(&r, by_gap)),
            ["GET /helix/users", "GET /helix/streams", "GetVersion"]
        );
        assert_eq!(
            keys(&sorted_calls(&r, by_gap.clicked(4))),
            ["GetVersion", "GET /helix/streams", "GET /helix/users"]
        );
        let by_name = Sort::default().clicked(0);
        assert_eq!(
            keys(&sorted_calls(&r, by_name)),
            ["GET /helix/streams", "GET /helix/users", "GetVersion"]
        );
        assert_eq!(by_name.heading(0), "呼叫 ▲");
        assert_eq!(by_name.heading(1), "次數");
    }

    #[test]
    fn cells_read_like_the_report() {
        assert_eq!(cell(&json!(null)), "");
        assert_eq!(cell(&json!(20.5)), "20.5");
        assert_eq!(cell(&json!([100, 200])), "100 200");
    }
}
