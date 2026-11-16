//! File status and line diffs between a snapshot and the current file.

use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Same,
    Different,
    OnlyInSnapshot,
    OnlyOnDisk,
    BothAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Removed(String),
    Added(String),
    Gap,
}

pub const MAX_DIFF_BYTES: usize = 1024 * 1024;

const CONTEXT_LINES: usize = 3;
const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";

pub fn status(snapshot: Option<&[u8]>, current: Option<&[u8]>) -> FileStatus {
    match (snapshot, current) {
        (None, None) => FileStatus::BothAbsent,
        (Some(_), None) => FileStatus::OnlyInSnapshot,
        (None, Some(_)) => FileStatus::OnlyOnDisk,
        (Some(old), Some(new)) if old == new => FileStatus::Same,
        (Some(_), Some(_)) => FileStatus::Different,
    }
}

fn decode(bytes: &[u8]) -> Option<&str> {
    if bytes.len() > MAX_DIFF_BYTES {
        return None;
    }
    let body = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    std::str::from_utf8(body).ok()
}

pub fn lines(snapshot: &[u8], current: &[u8]) -> Option<Vec<DiffLine>> {
    let old = decode(snapshot)?;
    let new = decode(current)?;
    let diff = TextDiff::from_lines(old, new);
    let mut out = Vec::new();
    for (index, group) in diff.grouped_ops(CONTEXT_LINES).iter().enumerate() {
        if index > 0 {
            out.push(DiffLine::Gap);
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let text = change.value().trim_end_matches(['\r', '\n']).to_string();
                out.push(match change.tag() {
                    ChangeTag::Equal => DiffLine::Context(text),
                    ChangeTag::Delete => DiffLine::Removed(text),
                    ChangeTag::Insert => DiffLine::Added(text),
                });
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> String {
        text.to_string()
    }

    #[test]
    fn status_covers_all_five_cases() {
        assert_eq!(status(Some(b"a"), Some(b"a")), FileStatus::Same);
        assert_eq!(status(Some(b"a"), Some(b"b")), FileStatus::Different);
        assert_eq!(status(Some(b"a"), None), FileStatus::OnlyInSnapshot);
        assert_eq!(status(None, Some(b"a")), FileStatus::OnlyOnDisk);
        assert_eq!(status(None, None), FileStatus::BothAbsent);
    }

    #[test]
    fn lines_marks_added_and_removed_with_context() {
        let diff = lines(b"a\nb\nc\n", b"a\nB\nc\n").unwrap();
        assert_eq!(
            diff,
            vec![
                DiffLine::Context(s("a")),
                DiffLine::Removed(s("b")),
                DiffLine::Added(s("B")),
                DiffLine::Context(s("c")),
            ]
        );
    }

    #[test]
    fn lines_separates_distant_hunks_with_gap() {
        let old: String = (1..=20).map(|n| format!("line{n}\n")).collect();
        let new = old
            .replace("line2\n", "two\n")
            .replace("line18\n", "eighteen\n");
        let diff = lines(old.as_bytes(), new.as_bytes()).unwrap();
        let gaps = diff.iter().filter(|l| **l == DiffLine::Gap).count();
        assert_eq!(gaps, 1);
        assert_eq!(diff.first(), Some(&DiffLine::Context(s("line1"))));
        assert_eq!(diff.last(), Some(&DiffLine::Context(s("line20"))));
    }

    #[test]
    fn lines_identical_is_empty() {
        assert_eq!(lines(b"a\nb\n", b"a\nb\n"), Some(vec![]));
    }

    #[test]
    fn lines_handles_crlf_without_showing_carriage_returns() {
        let diff = lines(b"a\r\nb\r\n", b"a\r\nc\r\n").unwrap();
        assert_eq!(
            diff,
            vec![
                DiffLine::Context(s("a")),
                DiffLine::Removed(s("b")),
                DiffLine::Added(s("c")),
            ]
        );
    }

    #[test]
    fn lines_none_for_non_utf8_or_oversize() {
        assert_eq!(lines(b"\xff\xfe\x00", b"a\n"), None);
        assert_eq!(lines(b"a\n", b"\xc3\x28"), None);
        let big = vec![b'a'; MAX_DIFF_BYTES + 1];
        assert_eq!(lines(&big, b"a\n"), None);
        assert_eq!(lines(b"a\n", &big), None);
    }

    #[test]
    fn lines_strips_bom() {
        let diff = lines(b"\xef\xbb\xbfa\n", b"a\n").unwrap();
        assert_eq!(diff, vec![]);
    }
}
