// =============================================================================
// Task Description Sections
// =============================================================================
//
// Tasks are stored as human-readable Markdown, but some workflows need a more
// structured view of that Markdown. In particular:
// - the task list wants a concise title
// - remote dispatch wants a clear separation between the model-shaped summary
//   and the original raw note from the user
//
// Instead of introducing a second persisted format, we keep one Markdown body
// contract and provide tolerant parsing helpers around a couple of headings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDescriptionSections {
    pub title: String,
    pub summary_markdown: Option<String>,
    pub original_note: Option<String>,
}

pub fn append_follow_up_request(
    description: &str,
    timestamp_label: &str,
    follow_up_request: &str,
) -> String {
    let normalized_description = description.trim();
    let normalized_follow_up_request = follow_up_request.trim();
    if normalized_follow_up_request.is_empty() {
        return normalized_description.to_owned();
    }

    let follow_up_block =
        format!("### {timestamp_label}\n\n{normalized_follow_up_request}");

    if normalized_description.contains("## Follow-up requests") {
        format!("{normalized_description}\n\n{follow_up_block}")
    } else if normalized_description.is_empty() {
        format!("## Follow-up requests\n\n{follow_up_block}")
    } else {
        format!("{normalized_description}\n\n## Follow-up requests\n\n{follow_up_block}")
    }
}

pub fn render_task_description(
    title: &str,
    summary_markdown: Option<&str>,
    original_note: Option<&str>,
) -> String {
    let normalized_title = title.trim();
    let normalized_summary = summary_markdown
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let normalized_original_note = original_note
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut sections = vec![normalized_title.to_owned()];
    if let Some(summary_markdown) = normalized_summary {
        sections.push(format!("## Summary\n\n{summary_markdown}"));
    }

    if let Some(original_note) = normalized_original_note {
        sections.push(format!(
            "## Original note\n\n{}",
            quote_as_blockquote(original_note)
        ));
    }

    sections.join("\n\n")
}

pub fn parse_task_description(description: &str) -> TaskDescriptionSections {
    let normalized = description.trim();
    let normalized_without_follow_ups = strip_markdown_section(normalized, "Follow-up requests");
    let mut lines = normalized.lines();
    let title = lines
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_owned())
        .unwrap_or_default();

    let summary_markdown = extract_markdown_section(normalized, "Summary");
    let original_note =
        extract_markdown_section(normalized, "Original note").map(unquote_blockquote);

    TaskDescriptionSections {
        title: if title.is_empty() {
            normalized.to_owned()
        } else {
            title
        },
        summary_markdown: summary_markdown.or_else(|| {
            if normalized_without_follow_ups.is_empty() {
                None
            } else {
                Some(normalized_without_follow_ups.to_owned())
            }
        }),
        original_note,
    }
}

fn extract_markdown_section(description: &str, heading: &str) -> Option<String> {
    let marker = format!("## {heading}");
    let start = description.find(&marker)?;
    let after_heading = &description[start + marker.len()..];
    let after_heading = after_heading.trim_start_matches([' ', '\t', '\r', '\n']);
    if after_heading.is_empty() {
        return None;
    }

    let end = after_heading.find("\n## ").unwrap_or(after_heading.len());
    let section = after_heading[..end].trim();
    if section.is_empty() {
        None
    } else {
        Some(section.to_owned())
    }
}

fn strip_markdown_section<'a>(description: &'a str, heading: &str) -> &'a str {
    let marker = format!("\n## {heading}");
    description
        .find(&marker)
        .map(|index| description[..index].trim_end())
        .unwrap_or(description)
}

fn quote_as_blockquote(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                ">".to_owned()
            } else {
                format!("> {}", line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unquote_blockquote(value: String) -> String {
    value
        .lines()
        .map(|line| {
            line.trim_start()
                .strip_prefix('>')
                .map(str::trim_start)
                .unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::{append_follow_up_request, parse_task_description, render_task_description};

    #[test]
    fn renders_summary_and_original_note_sections() {
        let description = render_task_description(
            "Fix a bug in module A",
            Some("- Inspect `module_a.rs`"),
            Some("proj-x prio high fix a bug in module A"),
        );

        assert_eq!(
            description,
            "Fix a bug in module A\n\n## Summary\n\n- Inspect `module_a.rs`\n\n## Original note\n\n> proj-x prio high fix a bug in module A"
        );
    }

    #[test]
    fn parses_task_description_sections_from_markdown() {
        let sections = parse_task_description(
            "Fix a bug in module A\n\n## Summary\n\n- Inspect `module_a.rs`\n\n## Original note\n\n> proj-x prio high fix a bug in module A",
        );

        assert_eq!(sections.title, "Fix a bug in module A");
        assert_eq!(
            sections.summary_markdown,
            Some("- Inspect `module_a.rs`".to_owned())
        );
        assert_eq!(
            sections.original_note,
            Some("proj-x prio high fix a bug in module A".to_owned())
        );
    }

    #[test]
    fn falls_back_to_the_full_body_when_sections_are_missing() {
        let sections = parse_task_description("Investigate flaky integration test");

        assert_eq!(sections.title, "Investigate flaky integration test");
        assert_eq!(
            sections.summary_markdown,
            Some("Investigate flaky integration test".to_owned())
        );
        assert_eq!(sections.original_note, None);
    }

    #[test]
    fn appends_follow_up_requests_without_overwriting_existing_context() {
        let updated = append_follow_up_request(
            "Fix a bug in module A\n\n## Summary\n\n- Inspect `module_a.rs`",
            "2026-03-18T14:00:00Z",
            "Address review comments on the PR.",
        );

        assert!(updated.contains("## Summary"));
        assert!(updated.contains("## Follow-up requests"));
        assert!(updated.contains("### 2026-03-18T14:00:00Z"));
        assert!(updated.contains("Address review comments on the PR."));
    }

    #[test]
    fn fallback_summary_ignores_follow_up_history() {
        let sections = parse_task_description(
            "Investigate flaky integration test\n\nNeed to inspect retry logic.\n\n## Follow-up requests\n\n### 2026-03-18T14:00:00Z\n\nAddress review comments.",
        );

        assert_eq!(sections.title, "Investigate flaky integration test");
        assert_eq!(
            sections.summary_markdown,
            Some("Investigate flaky integration test\n\nNeed to inspect retry logic.".to_owned())
        );
    }
}
