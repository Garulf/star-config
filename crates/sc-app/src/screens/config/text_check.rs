//! Checks text typed in the user.cfg text view before it counts as settings again.

/// `Err` names the first line the game would not read as a setting, a comment or a blank line.
pub fn check(text: &str) -> Result<(), String> {
    for (index, line) in text.lines().enumerate() {
        if let Some(problem) = problem(line.trim()) {
            return Err(format!("Line {}: {problem}", index + 1));
        }
    }
    Ok(())
}

fn problem(line: &str) -> Option<&'static str> {
    if line.is_empty() || line.starts_with(';') || line.starts_with("--") {
        return None;
    }
    let Some((name, _)) = line.split_once('=') else {
        return Some(
            "this is not a setting. Write it as name = value, or start it with ; to make it a comment.",
        );
    };
    let name = name.trim();
    if name.is_empty() {
        return Some("there is no name before the =.");
    }
    if name.contains(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ';')) {
        return Some("a name cannot contain spaces, quotes or ;.");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_comments_and_blank_lines_pass() {
        assert_eq!(
            check("; my settings\n\nr_DisplayInfo = 1\n-- old\ncl_fov=90\n"),
            Ok(())
        );
    }

    #[test]
    fn first_bad_line_is_named() {
        let err = check("a = 1\nnot a setting\n= 3\n").unwrap_err();
        assert!(err.starts_with("Line 2: this is not a setting."), "{err}");
        assert!(
            check("a = 1\n= 3\n")
                .unwrap_err()
                .starts_with("Line 2: there is no name")
        );
        assert!(
            check("r display = 1\n")
                .unwrap_err()
                .starts_with("Line 1: a name")
        );
    }
}
