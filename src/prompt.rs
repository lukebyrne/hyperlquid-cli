use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal,
};
use inquire::ui::{Attributes, Color, ErrorMessageRenderConfig, RenderConfig, StyleSheet, Styled};
use inquire::{Confirm, Select, Text};

use crate::output;

static PROMPT_THEME_INITIALIZED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

fn ensure_prompt_theme() {
    PROMPT_THEME_INITIALIZED.get_or_init(|| {
        if !output::colors_enabled() {
            inquire::set_global_render_config(RenderConfig::empty());
            return;
        }

        let mut config = RenderConfig::default()
            .with_prompt_prefix(Styled::new("?").with_fg(Color::DarkCyan))
            .with_answered_prompt_prefix(Styled::new("✔").with_fg(Color::DarkGreen))
            .with_canceled_prompt_indicator(Styled::new("✔").with_fg(Color::DarkGrey))
            .with_answer(StyleSheet::new().with_fg(Color::DarkCyan))
            .with_help_message(StyleSheet::new().with_fg(Color::DarkGrey))
            .with_error_message(
                ErrorMessageRenderConfig::empty()
                    .with_prefix(Styled::new("✖").with_fg(Color::DarkRed))
                    .with_message(StyleSheet::new().with_fg(Color::DarkRed)),
            )
            .with_highlighted_option_prefix(Styled::new("❯").with_fg(Color::DarkCyan))
            .with_selected_option(Some(StyleSheet::new().with_fg(Color::DarkCyan)));

        config.prompt = StyleSheet::new().with_attr(Attributes::BOLD);
        inquire::set_global_render_config(config);
    });
}

pub fn prompt(message: &str) -> Result<String> {
    ensure_prompt_theme();
    Ok(Text::new(message).prompt()?.trim().to_string())
}

pub fn confirm(message: &str, default: bool) -> Result<bool> {
    ensure_prompt_theme();
    Ok(Confirm::new(message).with_default(default).prompt()?)
}

pub struct SelectOption<T> {
    pub value: T,
    pub label: String,
}

pub fn select<T: Clone>(message: &str, options: Vec<SelectOption<T>>) -> Result<T> {
    ensure_prompt_theme();
    let labels: Vec<String> = options.iter().map(|o| o.label.clone()).collect();
    let selected = Select::new(message, labels).prompt()?;
    let idx = options
        .iter()
        .position(|o| o.label == selected)
        .unwrap_or(0);
    Ok(options[idx].value.clone())
}

pub fn press_enter_or_esc(message: &str) -> Result<bool> {
    println!("{} {message}", output::style_warning("→"));

    terminal::enable_raw_mode()?;
    let result = loop {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter => break Some(true),
                KeyCode::Esc => break Some(false),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    break None;
                }
                _ => {}
            }
        }
    };
    terminal::disable_raw_mode()?;

    match result {
        Some(true) => {
            println!("{} Opening browser...", output::style_profit("✔"));
            Ok(true)
        }
        Some(false) => {
            println!("{} Skipped", output::style_muted("✔"));
            Ok(false)
        }
        None => {
            // Match JS behavior: Ctrl+C exits immediately.
            println!();
            std::process::exit(0);
        }
    }
}
