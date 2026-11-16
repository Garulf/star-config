//! user.cfg parsing and editing.

use crate::error::{Error, Result};
use crate::staging::Document;
use std::collections::BTreeMap;
use std::path::Path;

const BOM: &[u8] = b"\xef\xbb\xbf";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    content: Vec<u8>,
    ending: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserCfg {
    bom: bool,
    lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub value: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Set { key: String, value: String },
    Remove { key: String },
    ReplaceText(String),
}

struct Assignment {
    key: String,
    value_start: usize,
    value_end: usize,
}

fn assignment(content: &[u8]) -> Option<Assignment> {
    let start = content.iter().position(|b| !is_blank(*b))?;
    let rest = &content[start..];
    if rest.starts_with(b";") || rest.starts_with(b"--") {
        return None;
    }
    let eq = start + rest.iter().position(|b| *b == b'=')?;
    let key = content[start..eq].trim_ascii_end();
    if key.is_empty() {
        return None;
    }
    let value_start = eq
        + 1
        + content[eq + 1..]
            .iter()
            .position(|b| !is_blank(*b))
            .unwrap_or(content.len() - eq - 1);
    let value_end = content.trim_ascii_end().len().max(value_start);
    Some(Assignment {
        key: String::from_utf8_lossy(key).into_owned(),
        value_start,
        value_end,
    })
}

fn is_blank(b: u8) -> bool {
    b.is_ascii_whitespace()
}

fn unquote(value: &[u8]) -> &[u8] {
    match value {
        [b'"', inner @ .., b'"'] => inner,
        _ => value,
    }
}

fn split_lines(bytes: &[u8]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let (body, tail) = match rest.iter().position(|b| *b == b'\n') {
            Some(i) => (&rest[..=i], &rest[i + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        let ending_len = if body.ends_with(b"\r\n") {
            2
        } else {
            usize::from(body.ends_with(b"\n"))
        };
        let split = body.len() - ending_len;
        lines.push(Line {
            content: body[..split].to_vec(),
            ending: body[split..].to_vec(),
        });
        rest = tail;
    }
    lines
}

fn invalid_key(key: &str) -> bool {
    key.is_empty()
        || key.starts_with("--")
        || key
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '=' | ';' | '"' | '\''))
}

impl UserCfg {
    fn from_bytes(bytes: &[u8], bom: bool) -> Self {
        Self {
            bom,
            lines: split_lines(bytes),
        }
    }

    pub fn entries(&self) -> Vec<Entry> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                let a = assignment(&line.content)?;
                let value = unquote(&line.content[a.value_start..a.value_end]);
                Some(Entry {
                    key: a.key,
                    value: String::from_utf8_lossy(value).into_owned(),
                    line: i + 1,
                })
            })
            .collect()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        let index = self.last_assignment(key)?;
        let line = &self.lines[index];
        let a = assignment(&line.content)?;
        let value = unquote(&line.content[a.value_start..a.value_end]);
        std::str::from_utf8(value).ok()
    }

    pub fn duplicates(&self) -> Vec<String> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for entry in self.entries() {
            *counts.entry(entry.key.to_lowercase()).or_default() += 1;
        }
        counts
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(k, _)| k)
            .collect()
    }

    /// The file as text for a raw view, or `None` when it is not UTF-8 and so cannot be shown
    /// and edited as text without changing bytes nobody touched.
    pub fn text(&self) -> Option<String> {
        String::from_utf8(self.body()).ok()
    }

    fn body(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for line in &self.lines {
            out.extend_from_slice(&line.content);
            out.extend_from_slice(&line.ending);
        }
        out
    }

    fn matches(line: &Line, key: &str) -> bool {
        assignment(&line.content).is_some_and(|a| a.key.eq_ignore_ascii_case(key))
    }

    fn last_assignment(&self, key: &str) -> Option<usize> {
        self.lines.iter().rposition(|line| Self::matches(line, key))
    }

    fn line_ending(&self) -> &'static [u8] {
        match self.lines.first() {
            None => b"\r\n",
            Some(first) if first.ending == b"\r\n" => b"\r\n",
            Some(_) => b"\n",
        }
    }

    fn set(&mut self, key: &str, value: &str) -> Result<()> {
        if invalid_key(key) {
            return Err(Error::Invalid(format!("invalid cvar name {key:?}")));
        }
        if value.contains(['\r', '\n']) {
            return Err(Error::Invalid("a value cannot span lines".into()));
        }
        if let Some(index) = self.last_assignment(key) {
            let content = &self.lines[index].content;
            let a = assignment(content).expect("matched line is an assignment");
            let old = &content[a.value_start..a.value_end];
            let quoted = old.len() >= 2 && old.starts_with(b"\"") && old.ends_with(b"\"");
            let mut next = content[..a.value_start].to_vec();
            if quoted {
                next.push(b'"');
            }
            next.extend_from_slice(value.as_bytes());
            if quoted {
                next.push(b'"');
            }
            next.extend_from_slice(&content[a.value_end..]);
            self.lines[index].content = next;
            return Ok(());
        }
        let ending = self.line_ending();
        if let Some(last) = self.lines.last_mut()
            && last.ending.is_empty()
        {
            last.ending = ending.to_vec();
        }
        self.lines.push(Line {
            content: format!("{key} = {value}").into_bytes(),
            ending: ending.to_vec(),
        });
        Ok(())
    }

    fn remove(&mut self, key: &str) -> Result<()> {
        let before = self.lines.len();
        self.lines.retain(|line| !Self::matches(line, key));
        if self.lines.len() == before {
            return Err(Error::Invalid(format!("{key} is not set")));
        }
        Ok(())
    }
}

impl Document for UserCfg {
    type Op = Op;

    fn parse(_path: &Path, bytes: &[u8]) -> Result<Self> {
        Ok(match bytes.strip_prefix(BOM) {
            Some(rest) => Self::from_bytes(rest, true),
            None => Self::from_bytes(bytes, false),
        })
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = if self.bom { BOM.to_vec() } else { Vec::new() };
        out.extend(self.body());
        out
    }

    fn apply(&mut self, op: &Op) -> Result<()> {
        match op {
            Op::Set { key, value } => self.set(key, value),
            Op::Remove { key } => self.remove(key),
            Op::ReplaceText(text) => {
                if self.text().is_none() {
                    return Err(Error::Invalid(
                        "user.cfg is not UTF-8, so it cannot be replaced from the text view".into(),
                    ));
                }
                *self = Self::from_bytes(text.as_bytes(), self.bom);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const FIXTURES: [&str; 4] = ["basic.cfg", "crlf_bom.cfg", "duplicates.cfg", "odd.cfg"];

    fn fixture(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/usercfg")
            .join(name);
        std::fs::read(path).unwrap()
    }

    fn parse(bytes: &[u8]) -> UserCfg {
        UserCfg::parse(Path::new("user.cfg"), bytes).unwrap()
    }

    fn set(key: &str, value: &str) -> Op {
        Op::Set {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn every_fixture_round_trips_byte_for_byte() {
        let mut inputs: Vec<Vec<u8>> = FIXTURES.iter().map(|n| fixture(n)).collect();
        inputs.push(b"a = 1\nb = 2".to_vec());
        inputs.push(b"\xef\xbb\xbfa = 1".to_vec());
        inputs.push(b"a = 1\r\nb = 2\nc = 3\r".to_vec());
        for bytes in inputs {
            assert_eq!(parse(&bytes).to_bytes(), bytes);
        }
    }

    #[test]
    fn empty_input_is_empty_document_and_renders_empty() {
        let cfg = parse(b"");
        assert!(cfg.entries().is_empty());
        assert!(cfg.to_bytes().is_empty());
    }

    #[test]
    fn entries_skip_comments_blank_and_unparsed_lines() {
        let got: Vec<(String, String, usize)> = parse(&fixture("basic.cfg"))
            .entries()
            .into_iter()
            .map(|e| (e.key, e.value, e.line))
            .collect();
        let want = [
            ("r_VSync", "1", 4),
            ("cl_fov", "108", 5),
            ("Sys_Spec", "3", 6),
            ("g_language", "english", 8),
        ];
        let want: Vec<(String, String, usize)> = want
            .iter()
            .map(|(k, v, l)| (k.to_string(), v.to_string(), *l))
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn get_is_case_insensitive_and_last_wins() {
        let cfg = parse(&fixture("duplicates.cfg"));
        assert_eq!(cfg.get("R_VSYNC"), Some("0"));
        assert_eq!(cfg.get("missing"), None);
    }

    #[test]
    fn duplicates_lists_repeated_keys() {
        assert_eq!(
            parse(&fixture("duplicates.cfg")).duplicates(),
            vec!["r_vsync".to_string()]
        );
        assert!(parse(&fixture("basic.cfg")).duplicates().is_empty());
    }

    #[test]
    fn quoted_value_is_unquoted_in_entry_but_kept_on_disk() {
        let bytes = fixture("odd.cfg");
        let cfg = parse(&bytes);
        assert_eq!(cfg.get("msg"), Some("a=b"));
        assert_eq!(cfg.entries().len(), 2);
        assert_eq!(cfg.to_bytes(), bytes);
    }

    #[test]
    fn set_existing_changes_only_the_value_bytes() {
        let mut cfg = parse(&fixture("crlf_bom.cfg"));
        cfg.apply(&set("R_VSYNC", "0")).unwrap();
        let mut want = fixture("crlf_bom.cfg");
        let at = want.windows(11).position(|w| w == b"r_VSync = 1").unwrap();
        want[at + 10] = b'0';
        assert_eq!(cfg.to_bytes(), want);

        let mut cfg = parse(&fixture("basic.cfg"));
        cfg.apply(&set("sys_spec", "4")).unwrap();
        let text = String::from_utf8(cfg.to_bytes()).unwrap();
        assert!(text.contains("\n  Sys_Spec   =   4\n"));
        cfg.apply(&set("cl_fov", "90")).unwrap();
        let text = String::from_utf8(cfg.to_bytes()).unwrap();
        assert!(text.contains("\ncl_fov=90\n"));
    }

    #[test]
    fn set_existing_keeps_quotes() {
        let mut cfg = parse(&fixture("odd.cfg"));
        cfg.apply(&set("msg", "x=y")).unwrap();
        let text = String::from_utf8_lossy(&cfg.to_bytes()).into_owned();
        assert!(text.contains("msg = \"x=y\"\n"));
        assert_eq!(cfg.get("msg"), Some("x=y"));
    }

    #[test]
    fn set_on_duplicate_key_changes_the_last_one() {
        let mut cfg = parse(&fixture("duplicates.cfg"));
        cfg.apply(&set("r_vsync", "2")).unwrap();
        assert_eq!(cfg.to_bytes(), b"r_VSync = 1\nother = x\nR_vsync = 2\n");
    }

    #[test]
    fn set_new_key_appends_with_file_line_ending() {
        let mut cfg = parse(&fixture("crlf_bom.cfg"));
        cfg.apply(&set("g_language", "english")).unwrap();
        let bytes = cfg.to_bytes();
        assert!(bytes.starts_with(b"\xef\xbb\xbf"));
        assert!(bytes.ends_with(b"g_language = english\r\n"));
    }

    #[test]
    fn set_new_key_on_file_without_final_newline_adds_one_first() {
        let mut cfg = parse(b"a = 1");
        cfg.apply(&set("b", "2")).unwrap();
        assert_eq!(cfg.to_bytes(), b"a = 1\nb = 2\n");
    }

    #[test]
    fn set_new_key_on_empty_file() {
        let mut cfg = parse(b"");
        cfg.apply(&set("g_language", "english")).unwrap();
        assert_eq!(cfg.to_bytes(), b"g_language = english\r\n");
    }

    #[test]
    fn set_rejects_bad_keys_and_multiline_values() {
        let mut cfg = parse(b"a = 1\n");
        for (key, value) in [
            ("", "1"),
            ("a b", "1"),
            ("a=b", "1"),
            ("a;b", "1"),
            ("a\nb", "1"),
            ("--x", "1"),
            ("a\"b", "1"),
            ("a'b", "1"),
            ("a\u{7}b", "1"),
            ("x", "1\n2"),
            ("x", "1\r2"),
        ] {
            assert!(matches!(
                cfg.apply(&set(key, value)),
                Err(crate::error::Error::Invalid(_))
            ));
        }
        assert_eq!(cfg.to_bytes(), b"a = 1\n");
    }

    #[test]
    fn remove_deletes_every_assignment_of_the_key() {
        let mut cfg = parse(&fixture("duplicates.cfg"));
        cfg.apply(&Op::Remove {
            key: "R_VSYNC".into(),
        })
        .unwrap();
        assert_eq!(cfg.to_bytes(), b"other = x\n");
    }

    #[test]
    fn remove_absent_key_is_invalid() {
        let mut cfg = parse(b"a = 1\n");
        let err = cfg.apply(&Op::Remove { key: "b".into() });
        assert!(matches!(err, Err(crate::error::Error::Invalid(_))));
    }

    #[test]
    fn replace_text_reparses_and_keeps_bom() {
        let mut cfg = parse(&fixture("crlf_bom.cfg"));
        cfg.apply(&Op::ReplaceText("x = 1\ny = 2".into())).unwrap();
        assert_eq!(cfg.to_bytes(), b"\xef\xbb\xbfx = 1\ny = 2");
        assert_eq!(cfg.get("y"), Some("2"));
        let mut plain = parse(b"a = 1\n");
        plain.apply(&Op::ReplaceText("b = 2\n".into())).unwrap();
        assert_eq!(plain.to_bytes(), b"b = 2\n");
    }

    #[test]
    fn text_is_the_body_without_the_bom_or_none_when_not_utf8() {
        let cfg = parse(&fixture("crlf_bom.cfg"));
        assert!(cfg.text().unwrap().starts_with("; header\r\n"));
        assert_eq!(parse(&fixture("odd.cfg")).text(), None);
    }

    #[test]
    fn replace_text_refuses_a_file_that_is_not_utf8() {
        let bytes = fixture("odd.cfg");
        let mut cfg = parse(&bytes);
        let result = cfg.apply(&Op::ReplaceText("a = 1\n".into()));
        assert!(matches!(result, Err(crate::error::Error::Invalid(_))));
        assert_eq!(cfg.to_bytes(), bytes);
    }

    #[test]
    fn set_key_is_read_back_as_that_key() {
        let mut cfg = parse(b"");
        cfg.apply(&set("r_DisplayInfo", "1")).unwrap();
        assert_eq!(cfg.get("r_DisplayInfo"), Some("1"));
        assert_eq!(cfg.entries().len(), 1);
    }

    #[test]
    fn invalid_utf8_in_untouched_line_survives_an_edit() {
        let mut cfg = parse(&fixture("odd.cfg"));
        cfg.apply(&set("k", "w")).unwrap();
        let bytes = cfg.to_bytes();
        assert!(bytes.windows(10).any(|w| w == b"; bad byte"));
        assert!(bytes.contains(&0xff));
        assert!(bytes.ends_with(b"k = w\n"));
    }
}
