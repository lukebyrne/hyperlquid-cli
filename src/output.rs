use std::{
    borrow::Cow,
    io::{self, IsTerminal, Write},
};

use anyhow::Result;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default)]
pub struct OutputOptions {
    pub json: bool,
}

pub fn format_json_pretty<T: Serialize>(data: &T) -> Result<String> {
    Ok(serde_json::to_string_pretty(data)?)
}

pub fn format_json_line<T: Serialize>(data: &T) -> Result<String> {
    Ok(serde_json::to_string(data)?)
}

pub fn print_json_pretty<T: Serialize>(data: &T) -> Result<()> {
    let s = format_json_pretty(data)?;
    println!("{s}");
    Ok(())
}

pub fn print_json_line<T: Serialize>(data: &T) -> Result<()> {
    let s = format_json_line(data)?;
    println!("{s}");
    Ok(())
}

pub fn print_success(message: impl AsRef<str>) {
    println!("{}", message.as_ref());
}

pub fn format_error(message: impl AsRef<str>) -> String {
    format!("Error: {}", message.as_ref())
}

pub fn print_error(message: impl AsRef<str>) {
    let _ = writeln!(io::stderr(), "{}", format_error(message));
}

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_UNDERLINE: &str = "\x1b[4m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_BLUE: &str = "\x1b[34m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_GRAY: &str = "\x1b[90m";

pub fn colors_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Some(force) = std::env::var_os("FORCE_COLOR")
        && force != "0"
    {
        return true;
    }
    if let Some(term) = std::env::var_os("TERM")
        && term == "dumb"
    {
        return false;
    }
    io::stdout().is_terminal()
}

fn ansi_wrap(text: impl AsRef<str>, prefixes: &[&str]) -> String {
    let text = text.as_ref();
    if !colors_enabled() || prefixes.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(
        prefixes.iter().map(|p| p.len()).sum::<usize>() + text.len() + ANSI_RESET.len(),
    );
    for p in prefixes {
        out.push_str(p);
    }
    out.push_str(text);
    out.push_str(ANSI_RESET);
    out
}

pub fn style_bold(text: impl AsRef<str>) -> String {
    ansi_wrap(text, &[ANSI_BOLD])
}

pub fn style_header(text: impl AsRef<str>) -> String {
    ansi_wrap(text, &[ANSI_BOLD, ANSI_CYAN])
}

pub fn style_link(text: impl AsRef<str>) -> String {
    ansi_wrap(text, &[ANSI_CYAN, ANSI_UNDERLINE])
}

pub fn style_muted(text: impl AsRef<str>) -> String {
    ansi_wrap(text, &[ANSI_GRAY])
}

pub fn style_profit(text: impl AsRef<str>) -> String {
    ansi_wrap(text, &[ANSI_GREEN])
}

pub fn style_profit_bold(text: impl AsRef<str>) -> String {
    ansi_wrap(text, &[ANSI_BOLD, ANSI_GREEN])
}

pub fn style_loss(text: impl AsRef<str>) -> String {
    ansi_wrap(text, &[ANSI_RED])
}

pub fn style_loss_bold(text: impl AsRef<str>) -> String {
    ansi_wrap(text, &[ANSI_BOLD, ANSI_RED])
}

pub fn style_warning(text: impl AsRef<str>) -> String {
    ansi_wrap(text, &[ANSI_YELLOW])
}

pub fn style_info(text: impl AsRef<str>) -> String {
    ansi_wrap(text, &[ANSI_BLUE])
}

pub fn format_short_address(address: &str) -> String {
    if address.len() == 42 && address.starts_with("0x") {
        format!("{}...{}", &address[..6], &address[address.len() - 4..])
    } else {
        address.to_string()
    }
}

#[derive(Clone, Copy, Debug)]
enum Align {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
pub enum TableAlign {
    Left,
    Right,
}

impl From<TableAlign> for Align {
    fn from(value: TableAlign) -> Self {
        match value {
            TableAlign::Left => Align::Left,
            TableAlign::Right => Align::Right,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TableColumn<'a> {
    pub header: &'a str,
    pub align: TableAlign,
    pub min_width: Option<usize>,
}

impl<'a> TableColumn<'a> {
    pub const fn left(header: &'a str) -> Self {
        Self {
            header,
            align: TableAlign::Left,
            min_width: None,
        }
    }

    pub const fn right(header: &'a str) -> Self {
        Self {
            header,
            align: TableAlign::Right,
            min_width: None,
        }
    }

    pub const fn left_with_width(header: &'a str, min_width: usize) -> Self {
        Self {
            header,
            align: TableAlign::Left,
            min_width: Some(min_width),
        }
    }

    pub const fn right_with_width(header: &'a str, min_width: usize) -> Self {
        Self {
            header,
            align: TableAlign::Right,
            min_width: Some(min_width),
        }
    }
}

fn strip_ansi_codes(s: &str) -> Cow<'_, str> {
    if !s.contains('\x1b') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // '['
            while let Some(c) = chars.next() {
                if c == 'm' {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    Cow::Owned(out)
}

fn visible_len(s: &str) -> usize {
    strip_ansi_codes(s).chars().count()
}

fn parse_numeric_like(s: &str) -> Option<f64> {
    let stripped = strip_ansi_codes(s);
    let mut s = stripped.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(stripped) = s.strip_suffix('%') {
        s = stripped.trim();
    }
    s = s.trim_start_matches('+');
    let s = s.replace(',', "");
    s.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn is_numeric_like(s: &str) -> bool {
    parse_numeric_like(s).is_some()
}

fn pad(value: &str, width: usize, align: Align) -> String {
    let len = visible_len(value);
    if len >= width {
        return value.to_string();
    }
    let pad = " ".repeat(width - len);
    match align {
        Align::Left => format!("{value}{pad}"),
        Align::Right => format!("{pad}{value}"),
    }
}

pub fn print_table(headers: &[&str], rows: Vec<Vec<String>>) {
    let s = format_table(headers, &rows);
    if !s.is_empty() {
        println!("{s}");
    }
}

pub fn print_table_with_columns(columns: &[TableColumn<'_>], rows: Vec<Vec<String>>) {
    let s = format_table_with_columns(columns, &rows);
    if !s.is_empty() {
        println!("{s}");
    }
}

pub fn format_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    if headers.is_empty() {
        return String::new();
    }
    if rows.is_empty() {
        return "(empty)".to_string();
    }

    let col_count = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| visible_len(h)).collect();
    for row in rows {
        for (i, width) in widths.iter_mut().enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            *width = (*width).max(visible_len(cell));
        }
    }

    let mut aligns = vec![Align::Left; col_count];
    for (i, align) in aligns.iter_mut().enumerate() {
        let mut any = false;
        let mut all_numeric = true;
        for row in rows {
            let cell_raw = row.get(i).map(|s| s.trim()).unwrap_or("");
            let cell_stripped = strip_ansi_codes(cell_raw);
            let cell = cell_stripped.trim();
            if cell.is_empty() || cell == "-" || cell == "?" || cell == "N/A" {
                continue;
            }
            any = true;
            if !is_numeric_like(cell) {
                all_numeric = false;
                break;
            }
        }
        if any && all_numeric {
            *align = Align::Right;
        }
    }

    let header_cells: Vec<String> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let padded = pad(h, widths[i], aligns[i]);
            if colors_enabled() {
                style_header(padded)
            } else {
                padded
            }
        })
        .collect();

    let dashes: Vec<String> = widths
        .iter()
        .map(|w| {
            let s = "-".repeat(*w);
            if colors_enabled() { style_muted(s) } else { s }
        })
        .collect();

    let mut out = String::new();
    out.push_str(&header_cells.join("  "));
    out.push('\n');
    out.push_str(&dashes.join("  "));

    for row in rows {
        out.push('\n');
        let mut cells = Vec::with_capacity(col_count);
        for (i, (width, align)) in widths.iter().zip(aligns.iter()).enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            cells.push(pad(cell, *width, *align));
        }
        out.push_str(&cells.join("  "));
    }
    out
}

pub fn format_table_with_columns(columns: &[TableColumn<'_>], rows: &[Vec<String>]) -> String {
    if columns.is_empty() {
        return String::new();
    }
    if rows.is_empty() {
        return "(empty)".to_string();
    }

    let col_count = columns.len();
    let mut widths: Vec<usize> = columns.iter().map(|c| visible_len(c.header)).collect();
    for (i, col) in columns.iter().enumerate() {
        if let Some(min_width) = col.min_width {
            widths[i] = widths[i].max(min_width);
        }
    }
    for row in rows {
        for (i, width) in widths.iter_mut().enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            *width = (*width).max(visible_len(cell));
        }
    }

    let header_cells: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let padded = pad(col.header, widths[i], col.align.into());
            if colors_enabled() {
                style_header(padded)
            } else {
                padded
            }
        })
        .collect();

    let dashes: Vec<String> = widths
        .iter()
        .map(|w| {
            let s = "-".repeat(*w);
            if colors_enabled() { style_muted(s) } else { s }
        })
        .collect();

    let mut out = String::new();
    out.push_str(&header_cells.join("  "));
    out.push('\n');
    out.push_str(&dashes.join("  "));

    for row in rows {
        out.push('\n');
        let mut cells = Vec::with_capacity(col_count);
        for (i, col) in columns.iter().enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            cells.push(pad(cell, widths[i], col.align.into()));
        }
        out.push_str(&cells.join("  "));
    }

    out
}

pub fn format_watch_header(title: impl AsRef<str>, last_updated: impl AsRef<str>) -> String {
    let left = format!(
        "{}{}",
        style_bold(title.as_ref()),
        style_muted(" (watching)")
    );
    let right = style_muted(format!("Last updated: {}", last_updated.as_ref()));

    let width = crossterm::terminal::size().ok().map(|(w, _)| w as usize);
    if let Some(width) = width {
        let left_len = visible_len(&left);
        let right_len = visible_len(&right);
        if width > left_len + right_len {
            let spaces = " ".repeat(width - left_len - right_len);
            return format!("{left}{spaces}{right}");
        }
    }

    format!("{left}  {right}")
}

pub fn format_human_lines(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Number(n) => vec![n.to_string()],
        serde_json::Value::Bool(b) => vec![b.to_string()],
        serde_json::Value::Null => vec!["null".to_string()],
        serde_json::Value::Array(arr) => format_human_array_lines(arr),
        serde_json::Value::Object(obj) => format_human_object_lines(obj, 0),
    }
}

fn format_human_array_lines(arr: &[serde_json::Value]) -> Vec<String> {
    if arr.is_empty() {
        return vec!["(empty)".to_string()];
    }

    if arr
        .first()
        .is_some_and(|first| matches!(first, serde_json::Value::Object(_)))
    {
        return format_array_of_objects_as_table(arr);
    }

    arr.iter()
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect()
}

fn value_to_table_cell(value: Option<&serde_json::Value>) -> String {
    match value {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        Some(other) => other.to_string(),
    }
}

fn format_array_of_objects_as_table(arr: &[serde_json::Value]) -> Vec<String> {
    let Some(first_obj) = arr.first().and_then(|v| v.as_object()) else {
        return vec!["(empty)".to_string()];
    };
    let headers: Vec<String> = first_obj.keys().cloned().collect();

    let rows: Vec<Vec<String>> = arr
        .iter()
        .map(|row| {
            let obj = row.as_object();
            headers
                .iter()
                .map(|key| value_to_table_cell(obj.and_then(|o| o.get(key))))
                .collect()
        })
        .collect();

    let header_refs: Vec<&str> = headers.iter().map(|s| s.as_str()).collect();
    format_table(&header_refs, &rows)
        .lines()
        .map(|s| s.to_string())
        .collect()
}

fn format_human_object_lines(
    obj: &serde_json::Map<String, serde_json::Value>,
    indent: usize,
) -> Vec<String> {
    let prefix = "  ".repeat(indent);
    let mut lines = Vec::new();

    for (key, value) in obj.iter() {
        match value {
            serde_json::Value::Object(child) => {
                lines.push(format!("{prefix}{key}:"));
                lines.extend(format_human_object_lines(child, indent + 1));
            }
            serde_json::Value::Array(arr) => {
                lines.push(format!("{prefix}{key}: [{} items]", arr.len()));
            }
            serde_json::Value::String(s) => lines.push(format!("{prefix}{key}: {s}")),
            serde_json::Value::Number(n) => lines.push(format!("{prefix}{key}: {n}")),
            serde_json::Value::Bool(b) => lines.push(format!("{prefix}{key}: {b}")),
            serde_json::Value::Null => lines.push(format!("{prefix}{key}: null")),
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_json_matches_js_json_stringify_indented() {
        let data = serde_json::json!({ "foo": "bar", "num": 42 });
        assert_eq!(
            format_json_pretty(&data).unwrap(),
            serde_json::to_string_pretty(&data).unwrap()
        );

        let arr = serde_json::json!([{ "a": 1 }, { "a": 2 }]);
        assert_eq!(
            format_json_pretty(&arr).unwrap(),
            serde_json::to_string_pretty(&arr).unwrap()
        );

        assert_eq!(format_json_pretty(&"hello").unwrap(), "\"hello\"");
        assert_eq!(format_json_pretty(&123).unwrap(), "123");
        let null: Option<String> = None;
        assert_eq!(format_json_pretty(&null).unwrap(), "null");
    }

    #[test]
    fn format_human_strings_arrays_objects() {
        assert_eq!(
            format_human_lines(&serde_json::Value::String("hello world".to_string())),
            vec!["hello world".to_string()]
        );

        assert_eq!(
            format_human_lines(&serde_json::json!([])),
            vec!["(empty)".to_string()]
        );

        assert_eq!(
            format_human_lines(&serde_json::json!(["a", "b", "c"])),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );

        let table_lines = format_human_lines(&serde_json::json!([
            { "coin": "BTC", "price": "50000" },
            { "coin": "ETH", "price": "3000" }
        ]));
        assert!(table_lines.len() >= 4);
        assert!(table_lines[0].contains("coin"));
        assert!(table_lines[0].contains("price"));
        assert!(table_lines[1].contains('-'));
        assert!(table_lines.iter().any(|l| l.contains("BTC")));
        assert!(table_lines.iter().any(|l| l.contains("3000")));

        // null/missing values should not panic
        let _ = format_human_lines(&serde_json::json!([
            { "a": "x", "b": null },
            { "a": "y" }
        ]));

        let lines = format_human_lines(&serde_json::json!({ "name": "test", "value": 42 }));
        assert_eq!(
            lines,
            vec!["name: test".to_string(), "value: 42".to_string()]
        );

        let nested = serde_json::json!({
            "outer": "value",
            "nested": { "inner": "data" }
        });
        let lines = format_human_lines(&nested);
        assert_eq!(
            lines,
            vec![
                "outer: value".to_string(),
                "nested:".to_string(),
                "  inner: data".to_string()
            ]
        );

        let lines = format_human_lines(&serde_json::json!({ "items": [1, 2, 3] }));
        assert_eq!(lines, vec!["items: [3 items]".to_string()]);

        let deep = serde_json::json!({ "level1": { "level2": { "level3": "deep" } } });
        let lines = format_human_lines(&deep);
        assert_eq!(
            lines,
            vec![
                "level1:".to_string(),
                "  level2:".to_string(),
                "    level3: deep".to_string()
            ]
        );
    }

    #[test]
    fn format_error_prefixes_message() {
        assert_eq!(
            format_error("something went wrong"),
            "Error: something went wrong"
        );
        assert_eq!(format_error(""), "Error: ");
    }

    #[test]
    fn visible_len_ignores_sgr_ansi_sequences() {
        assert_eq!(visible_len("hello"), 5);
        assert_eq!(visible_len("\x1b[31mhello\x1b[0m"), 5);
        assert_eq!(visible_len("\x1b[1m\x1b[36mhi\x1b[0m"), 2);
    }

    #[test]
    fn pad_uses_visible_len_for_alignment() {
        let value = "\x1b[31m1\x1b[0m";
        let padded_right = pad(value, 3, Align::Right);
        assert_eq!(strip_ansi_codes(&padded_right).chars().count(), 3);
        assert!(strip_ansi_codes(&padded_right).ends_with('1'));

        let padded_left = pad(value, 3, Align::Left);
        assert_eq!(strip_ansi_codes(&padded_left).chars().count(), 3);
        assert!(strip_ansi_codes(&padded_left).starts_with('1'));
    }

    #[test]
    fn parse_numeric_like_strips_ansi() {
        assert_eq!(parse_numeric_like("\x1b[32m+1.23%\x1b[0m"), Some(1.23));
        assert_eq!(parse_numeric_like("\x1b[31m-1,234.5\x1b[0m"), Some(-1234.5));
    }
}
