#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LineKind {
    Heading,
    Plain,
    Code,
    Checkbox(bool),
}

pub(crate) fn markup_body(body: &str) -> Vec<(LineKind, String)> {
    let mut in_code_block = false;
    let mut lines = Vec::new();

    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            lines.push((LineKind::Code, line.to_string()));
            continue;
        }

        if let Some(text) = heading_text(line) {
            lines.push((LineKind::Heading, strip_inline_marks(text)));
        } else if let Some((checked, text)) = checkbox_text(line) {
            lines.push((LineKind::Checkbox(checked), strip_inline_marks(text)));
        } else {
            lines.push((LineKind::Plain, strip_inline_marks(line)));
        }
    }

    lines
}

fn heading_text(line: &str) -> Option<&str> {
    let marker_count = line.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&marker_count) {
        return None;
    }

    let rest = &line[marker_count..];
    rest.chars()
        .next()
        .is_some_and(char::is_whitespace)
        .then(|| rest.trim_start())
}

fn checkbox_text(line: &str) -> Option<(bool, &str)> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("- [")?;
    let mut chars = rest.chars();
    let marker = chars.next()?;
    if !matches!(marker, ' ' | 'x' | 'X') {
        return None;
    }
    let rest = chars.as_str().strip_prefix(']')?;
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }
    Some((matches!(marker, 'x' | 'X'), rest.trim_start()))
}

fn strip_inline_marks(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut index = 0;

    while index < text.len() {
        let rest = &text[index..];
        if let Some(code) = code_span(rest) {
            stripped.push_str(code.text);
            index += code.consumed;
        } else {
            let ch = rest
                .chars()
                .next()
                .expect("non-empty slice should have a char");
            stripped.push(ch);
            index += ch.len_utf8();
        }
    }

    stripped
}

struct CodeSpan<'a> {
    text: &'a str,
    consumed: usize,
}

fn code_span(text: &str) -> Option<CodeSpan<'_>> {
    let rest = text.strip_prefix('`')?;
    let close = rest.find('`')?;
    Some(CodeSpan {
        text: &rest[..close],
        consumed: close + 2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_lines_become_heading_kind_without_marks() {
        let lines = markup_body("## 계획 (Planning)\n본문");
        assert_eq!(lines[0], (LineKind::Heading, "계획 (Planning)".to_string()));
        assert_eq!(lines[1], (LineKind::Plain, "본문".to_string()));
    }

    #[test]
    fn checkbox_lines_become_checkbox_kind() {
        let lines = markup_body("- [ ] Step 1\n- [x] Step 2");
        assert_eq!(lines[0], (LineKind::Checkbox(false), "Step 1".to_string()));
        assert_eq!(lines[1], (LineKind::Checkbox(true), "Step 2".to_string()));
    }

    #[test]
    fn uppercase_checked_marker_is_recognized() {
        let lines = markup_body("- [X] Step 1");
        assert_eq!(lines[0], (LineKind::Checkbox(true), "Step 1".to_string()));
    }

    #[test]
    fn fenced_block_lines_become_code_kind() {
        let lines = markup_body("```rust\nlet x = 1;\n```");
        assert!(
            lines
                .iter()
                .any(|(kind, text)| *kind == LineKind::Code && text == "let x = 1;")
        );
        assert!(!lines.iter().any(|(_, text)| text.contains("```")));
    }

    #[test]
    fn double_asterisks_are_preserved_and_code_marks_are_stripped() {
        let lines = markup_body("**중요** 그리고 `코드`");
        assert_eq!(lines[0].1, "**중요** 그리고 코드");
    }

    #[test]
    fn non_bold_double_asterisks_are_preserved() {
        let lines = markup_body("src/**/*.rs\na ** b");
        assert_eq!(lines[0].1, "src/**/*.rs");
        assert_eq!(lines[1].1, "a ** b");
    }

    #[test]
    fn multiple_glob_double_asterisks_are_preserved() {
        let lines = markup_body("src/**/*.rs tests/**/*.rs");
        assert_eq!(lines[0].1, "src/**/*.rs tests/**/*.rs");
    }

    #[test]
    fn double_asterisks_inside_code_spans_are_preserved() {
        let lines = markup_body("`src/**/*.rs` and `**literal**`");
        assert_eq!(lines[0].1, "src/**/*.rs and **literal**");
    }

    #[test]
    fn empty_body_yields_no_lines() {
        assert!(markup_body("").is_empty());
    }

    #[test]
    fn markup_body_does_not_panic_on_malformed_markup() {
        for body in [
            "```rust\nno close",
            "- [",
            "######",
            "**bold without close",
            "`",
        ] {
            let _ = markup_body(body);
        }
    }
}
