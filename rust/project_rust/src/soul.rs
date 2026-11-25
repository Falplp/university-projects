use std::{fs, path::Path};

pub fn load(path: impl AsRef<Path>) -> Option<String> {
    let path = path.as_ref();
    fs::read_to_string(path)
        .or_else(|_| fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)))
        .ok()
}

pub fn preview(path: impl AsRef<Path>, max_chars: usize) -> String {
    let Some(content) = load(path) else {
        return "SOUL.md not found".into();
    };
    let one_line = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("SOUL.md is empty")
        .trim();
    if one_line.chars().count() <= max_chars {
        one_line.into()
    } else {
        format!(
            "{}…",
            one_line
                .chars()
                .take(max_chars.saturating_sub(1))
                .collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_soul_from_workspace() {
        let content = load("souls/code-generator.md").unwrap();
        assert!(content.contains("approved specification"));
    }

    #[test]
    fn missing_soul_has_explanatory_preview() {
        assert_eq!(preview("souls/does-not-exist.md", 20), "SOUL.md not found");
    }

    #[test]
    fn preview_is_short_and_uses_first_non_empty_line() {
        let preview = preview("souls/code-reviewer.md", 18);
        assert!(preview.len() <= 21);
        assert!(preview.starts_with("# Code Reviewer"));
    }
}
