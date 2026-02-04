use chrono::Local;

pub const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";
pub const HIDE_CURSOR: &str = "\x1b[?25l";
pub const SHOW_CURSOR: &str = "\x1b[?25h";

pub fn clear_screen() {
    print!("{CLEAR_SCREEN}");
}

pub fn hide_cursor() {
    print!("{HIDE_CURSOR}");
}

pub fn show_cursor() {
    print!("{SHOW_CURSOR}");
}

pub fn format_timestamp() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_js_watch_ansi_sequences() {
        assert_eq!(CLEAR_SCREEN, "\x1b[2J\x1b[H");
        assert_eq!(HIDE_CURSOR, "\x1b[?25l");
        assert_eq!(SHOW_CURSOR, "\x1b[?25h");
    }

    #[test]
    fn format_timestamp_hh_mm_ss() {
        let ts = format_timestamp();
        let parts: Vec<_> = ts.split(':').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].len(), 2);
        assert_eq!(parts[1].len(), 2);
        assert_eq!(parts[2].len(), 2);

        let hours: u32 = parts[0].parse().unwrap();
        let minutes: u32 = parts[1].parse().unwrap();
        let seconds: u32 = parts[2].parse().unwrap();

        assert!(hours <= 23);
        assert!(minutes <= 59);
        assert!(seconds <= 59);
    }
}
