use std::env;
use std::io::{self, IsTerminal};

use owo_colors::OwoColorize;

#[derive(Clone, Copy)]
pub enum SummaryTone {
    Success,
    Info,
}

#[derive(Clone, Copy)]
pub enum ValueTone {
    Plain,
    Path,
    PriorityHigh,
    PriorityMedium,
    PriorityLow,
    StatusOpen,
    StatusClosed,
}

pub fn format_summary(
    title: &str,
    tone: SummaryTone,
    rows: &[(&str, String, ValueTone)],
) -> String {
    let mut lines = vec![style_title(title, tone)];
    let label_width = rows
        .iter()
        .map(|(label, _, _)| label.len())
        .max()
        .unwrap_or(0);

    for (label, value, value_tone) in rows {
        lines.push(format_summary_row(label, value, *value_tone, label_width));
    }

    lines.join("\n")
}

pub fn format_note(label: &str, value: &str) -> String {
    let raw_label = format!("{label:<8}");
    if should_color() {
        format!("  {}  {}", raw_label.bold().blue(), value.dimmed())
    } else {
        format!("  {raw_label}  {value}")
    }
}

pub fn format_prompt_label(label: &str, default_value: Option<&str>) -> String {
    match default_value.filter(|value| !value.trim().is_empty()) {
        Some(default_value) => format!("{label} [{default_value}]"),
        None => label.to_owned(),
    }
}

fn format_summary_row(
    label: &str,
    value: &str,
    value_tone: ValueTone,
    label_width: usize,
) -> String {
    let raw_label = format!("{label:<label_width$}", label_width = label_width);
    let rendered_label = if should_color() {
        raw_label.bold().blue().to_string()
    } else {
        raw_label
    };

    format!("  {rendered_label}  {}", style_value(value, value_tone))
}

fn style_title(title: &str, tone: SummaryTone) -> String {
    if !should_color() {
        return title.to_owned();
    }

    match tone {
        SummaryTone::Success => title.bold().green().to_string(),
        SummaryTone::Info => title.bold().cyan().to_string(),
    }
}

fn style_value(value: &str, tone: ValueTone) -> String {
    if !should_color() {
        return value.to_owned();
    }

    match tone {
        ValueTone::Plain => value.to_owned(),
        ValueTone::Path => value.cyan().to_string(),
        ValueTone::PriorityHigh => value.bold().red().to_string(),
        ValueTone::PriorityMedium => value.bold().yellow().to_string(),
        ValueTone::PriorityLow => value.bold().green().to_string(),
        ValueTone::StatusOpen => value.bold().green().to_string(),
        ValueTone::StatusClosed => value.bold().magenta().to_string(),
    }
}

fn should_color() -> bool {
    env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use std::io::IsTerminal as _;

    use super::{format_note, format_prompt_label, format_summary, SummaryTone, ValueTone};

    #[test]
    fn renders_plain_prompt_labels_with_defaults() {
        assert_eq!(
            format_prompt_label("Model file", Some("~/.models/parser.gguf")),
            "Model file [~/.models/parser.gguf]"
        );
    }

    // TODO: Tests that depend on this are questionable, do we want them?
    fn ensure_no_terminal<F: FnOnce()>(test: F) {
        static GUARD: std::sync::LazyLock<std::sync::Mutex<()>> =
            std::sync::LazyLock::new(|| std::sync::Mutex::new(()));
        let _lock = GUARD.lock().unwrap();

        // Make test pass if it's actually runs from a terminal
        if std::io::stdout().is_terminal() {
            unsafe { std::env::set_var("NO_COLOR", "1") };
        }

        test();

        if std::io::stdout().is_terminal() {
            unsafe { std::env::remove_var("NO_COLOR") };
        }
    }

    #[test]
    fn renders_plain_note_without_a_terminal() {
        ensure_no_terminal(|| {
            assert_eq!(
                format_note("Enter", "keep current values"),
                "  Enter     keep current values"
            );
        });
    }

    #[test]
    fn renders_plain_summary_without_a_terminal() {
        ensure_no_terminal(|| {
            let rendered = format_summary(
                "Created task",
                SummaryTone::Success,
                &[
                    ("Project", "project-x".to_owned(), ValueTone::Plain),
                    (
                        "File",
                        "~/.track/issues/project-x/open/task.md".to_owned(),
                        ValueTone::Path,
                    ),
                ],
            );

            assert_eq!(
                rendered,
                "Created task\n  Project  project-x\n  File     ~/.track/issues/project-x/open/task.md"
            );
        });
    }
}
