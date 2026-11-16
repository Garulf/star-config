//! attributes.xml parsing and editing.

use crate::error::{Error, Result};
use crate::staging::Document;
use crate::xml_edit::{self, Element, Scan, line_at};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const BOM: &[u8] = b"\xef\xbb\xbf";
const ROOT: &str = "Attributes";
const CHILD: &str = "Attr";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attributes {
    path: PathBuf,
    bom: bool,
    text: String,
    entries: Vec<AttrEntry>,
    version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrEntry {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Set { name: String, value: String },
    Remove { name: String },
}

impl Attributes {
    pub fn entries(&self) -> &[AttrEntry] {
        &self.entries
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.value.as_str())
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    fn from_text(path: PathBuf, bom: bool, text: String) -> Result<Self> {
        let layout = Layout::check(&path, &text)?;
        let entries = layout
            .attrs
            .iter()
            .map(|&index| {
                let element = &layout.scan.elements[index];
                AttrEntry {
                    name: attr_value(element, "name").unwrap_or_default().into(),
                    value: attr_value(element, "value").unwrap_or_default().into(),
                }
            })
            .collect();
        let version = layout
            .root
            .and_then(|root| attr_value(&layout.scan.elements[root], "Version"))
            .map(str::to_string);
        Ok(Self {
            path,
            bom,
            text,
            entries,
            version,
        })
    }

    fn edited(&self, op: &Op) -> Result<String> {
        let layout = Layout::check(&self.path, &self.text)?;
        let mut text = self.text.clone();
        match op {
            Op::Set { name, value } => {
                check_name(name)?;
                if value.contains('\0') {
                    return Err(Error::Invalid(format!(
                        "the value for {name} contains a NUL character"
                    )));
                }
                let existing = layout
                    .find(name)
                    .and_then(|element| element.attrs.iter().find(|attr| attr.name == "value"));
                let line = new_attr(name, value);
                match (existing, layout.root) {
                    (Some(attr), _) => xml_edit::set_attr(&mut text, attr, value),
                    (None, Some(root)) => {
                        xml_edit::append_child(&mut text, &layout.scan.elements[root], &line)
                    }
                    (None, None) => text.push_str(&format!(
                        "<{ROOT} Version=\"1\">\r\n {line}\r\n</{ROOT}>\r\n"
                    )),
                }
            }
            Op::Remove { name } => {
                check_name(name)?;
                let element = layout
                    .find(name)
                    .ok_or_else(|| Error::Invalid(format!("{name} is not in attributes.xml")))?;
                xml_edit::remove_element(&mut text, element);
            }
        }
        Ok(text)
    }
}

impl Document for Attributes {
    type Op = Op;

    fn parse(path: &Path, bytes: &[u8]) -> Result<Self> {
        let (bom, body) = match bytes.strip_prefix(BOM) {
            Some(body) => (true, body),
            None => (false, bytes),
        };
        let text = String::from_utf8(body.to_vec()).map_err(|err| {
            let valid = &body[..err.utf8_error().valid_up_to()];
            Error::Parse {
                path: path.to_path_buf(),
                line: valid.iter().filter(|&&b| b == b'\n').count() + 1,
                message: "the file is not valid UTF-8".into(),
            }
        })?;
        Self::from_text(path.to_path_buf(), bom, text)
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(BOM.len() + self.text.len());
        if self.bom {
            bytes.extend_from_slice(BOM);
        }
        bytes.extend_from_slice(self.text.as_bytes());
        bytes
    }

    fn apply(&mut self, op: &Op) -> Result<()> {
        let text = self.edited(op)?;
        *self = Self::from_text(self.path.clone(), self.bom, text)?;
        Ok(())
    }
}

/// A scan that passed the structure check, with the root and the `Attr` children located.
struct Layout {
    scan: Scan,
    root: Option<usize>,
    attrs: Vec<usize>,
}

impl Layout {
    fn check(path: &Path, text: &str) -> Result<Self> {
        let scan = xml_edit::scan(path, text)?;
        if text.is_empty() {
            return Ok(Self {
                scan,
                root: None,
                attrs: Vec::new(),
            });
        }
        let fail = |at: usize, reason: String| Error::Parse {
            path: path.to_path_buf(),
            line: line_at(text, at),
            message: format!("format not recognised: {reason}"),
        };
        if scan.has_doctype {
            let at = text.find("<!DOCTYPE").unwrap_or(0);
            return Err(fail(at, "the file has a DOCTYPE declaration".into()));
        }
        let mut roots = scan
            .elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.depth == 0);
        let Some((root, root_element)) = roots.next() else {
            return Err(fail(0, "there is no root element".into()));
        };
        if let Some((_, second)) = roots.next() {
            return Err(fail(
                second.open_tag.start,
                "there is more than one root element".into(),
            ));
        }
        if root_element.name != ROOT {
            return Err(fail(
                root_element.open_tag.start,
                format!(
                    "the root element is <{}>, expected <{ROOT}>",
                    root_element.name
                ),
            ));
        }

        let mut seen: HashMap<&str, usize> = HashMap::new();
        let mut attrs = Vec::new();
        for (index, element) in scan.elements.iter().enumerate().skip(root + 1) {
            if element.depth != 1 {
                continue;
            }
            let at = element.open_tag.start;
            if element.name != CHILD {
                return Err(fail(
                    at,
                    format!("unexpected element <{}> inside <{ROOT}>", element.name),
                ));
            }
            if let Some(child) = scan.elements.iter().find(|e| e.parent == Some(index)) {
                return Err(fail(
                    child.open_tag.start,
                    format!("<{}> is nested inside <{CHILD}>", child.name),
                ));
            }
            if element.close_tag.is_some() {
                return Err(fail(
                    at,
                    format!("<{CHILD}> has content, expected an empty element ending in />"),
                ));
            }
            let Some(name) = attr_value(element, "name") else {
                return Err(fail(at, format!("<{CHILD}> has no name attribute")));
            };
            if attr_value(element, "value").is_none() {
                return Err(fail(
                    at,
                    format!("<{CHILD} name=\"{name}\"> has no value attribute"),
                ));
            }
            if let Some(first) = seen.insert(name, line_at(text, at)) {
                return Err(fail(
                    at,
                    format!("the name \"{name}\" appears twice (first on line {first})"),
                ));
            }
            attrs.push(index);
        }

        let children: Vec<&Element> = attrs.iter().map(|&i| &scan.elements[i]).collect();
        if let Some(at) = stray_text(text, root_element, &children) {
            return Err(fail(at, format!("text outside an <{CHILD}> element")));
        }
        Ok(Self {
            scan,
            root: Some(root),
            attrs,
        })
    }

    fn find(&self, name: &str) -> Option<&Element> {
        self.attrs
            .iter()
            .map(|&index| &self.scan.elements[index])
            .find(|element| attr_value(element, "name") == Some(name))
    }
}

/// Finds character data in the gaps around the root and between its children.
fn stray_text(text: &str, root: &Element, children: &[&Element]) -> Option<usize> {
    let mut gaps = vec![0..root.span.start, root.span.end..text.len()];
    if let Some(close) = &root.close_tag {
        let mut from = root.open_tag.end;
        for child in children {
            gaps.push(from..child.span.start);
            from = child.span.end;
        }
        gaps.push(from..close.start);
    }
    gaps.into_iter()
        .find_map(|gap| first_text(&text[gap.clone()]).map(|at| gap.start + at))
}

fn first_text(gap: &str) -> Option<usize> {
    let mut at = 0;
    loop {
        at += gap[at..].len() - gap[at..].trim_start().len();
        let rest = &gap[at..];
        let skipped = if rest.is_empty() {
            return None;
        } else if rest.starts_with("<!--") {
            rest.find("-->").map(|end| end + 3)
        } else if rest.starts_with("<?") {
            rest.find("?>").map(|end| end + 2)
        } else {
            None
        };
        match skipped {
            Some(length) => at += length,
            None => return Some(at),
        }
    }
}

fn attr_value<'a>(element: &'a Element, name: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find(|attr| attr.name == name)
        .map(|attr| attr.value.as_str())
}

fn new_attr(name: &str, value: &str) -> String {
    format!(
        "<{CHILD} name=\"{}\" value=\"{}\"/>",
        xml_edit::escape_attr(name),
        xml_edit::escape_attr(value)
    )
}

fn check_name(name: &str) -> Result<()> {
    let bad = |c: char| matches!(c, '"' | '\'' | '<' | '>' | '&') || c.is_whitespace();
    if name.is_empty() || name.contains(bad) {
        return Err(Error::Invalid(format!(
            "{name:?} is not a usable attribute name"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::staging::Document;
    use std::fs;
    use std::path::Path;

    const RECOGNISED: [&str; 5] = [
        "basic.xml",
        "lf_tabs.xml",
        "bom.xml",
        "escapes.xml",
        "single_quotes.xml",
    ];
    const BOM: &[u8] = b"\xef\xbb\xbf";

    fn fixture(name: &str) -> Vec<u8> {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/attributes")
                .join(name),
        )
        .unwrap()
    }

    fn parse(bytes: &[u8]) -> crate::error::Result<Attributes> {
        Attributes::parse(Path::new("attributes.xml"), bytes)
    }

    fn set(name: &str, value: &str) -> Op {
        Op::Set {
            name: name.into(),
            value: value.into(),
        }
    }

    fn remove(name: &str) -> Op {
        Op::Remove { name: name.into() }
    }

    fn applied(bytes: &[u8], op: Op) -> Vec<u8> {
        let mut doc = parse(bytes).unwrap();
        doc.apply(&op).unwrap();
        doc.to_bytes()
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn not_recognised(bytes: &[u8]) -> (usize, String) {
        match parse(bytes).unwrap_err() {
            Error::Parse { line, message, .. } => (line, message),
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    #[test]
    fn every_recognised_fixture_round_trips_byte_for_byte() {
        for name in RECOGNISED {
            let bytes = fixture(name);
            assert_eq!(parse(&bytes).unwrap().to_bytes(), bytes, "{name}");
        }
        for inline in [
            &b"<Attributes Version=\"1\">\n <Attr name=\"A\" value=\"1\"/>\n</Attributes>"[..],
            b"\xef\xbb\xbf<Attributes/>",
            b"\xef\xbb\xbf",
        ] {
            assert_eq!(parse(inline).unwrap().to_bytes(), inline);
        }
    }

    #[test]
    fn entries_are_in_file_order_and_unescaped() {
        let doc = parse(&fixture("basic.xml")).unwrap();
        assert_eq!(doc.entries().len(), 12);
        assert_eq!(
            doc.entries()[0],
            AttrEntry {
                name: "HeadtrackingSource".into(),
                value: "0".into()
            }
        );
        assert_eq!(doc.entries()[11].name, "FOV");
        assert_eq!(doc.get("HeadtrackingGlobalSmoothingRotation"), Some("0.5"));
        assert_eq!(doc.get("fov"), None);
        assert_eq!(doc.version(), Some("1"));

        let doc = parse(&fixture("lf_tabs.xml")).unwrap();
        let names: Vec<_> = doc.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["HeadtrackingSource", "HeadtrackingToggle", "Brightness"]
        );
        assert_eq!(doc.version(), Some("2"));

        let doc = parse(&fixture("escapes.xml")).unwrap();
        assert_eq!(doc.get("Callsign"), Some("Tom & Jerry \"TJ\" <3"));

        let doc = parse(&fixture("single_quotes.xml")).unwrap();
        assert_eq!(doc.get("Callsign"), Some("it's \"me\""));
        assert_eq!(doc.get("HeadtrackingToggle"), Some("1"));
    }

    #[test]
    fn empty_input_is_empty_document_and_renders_empty() {
        let doc = parse(b"").unwrap();
        assert!(doc.entries().is_empty());
        assert_eq!(doc.version(), None);
        assert!(doc.to_bytes().is_empty());
    }

    #[test]
    fn set_existing_changes_only_that_value() {
        let bytes = fixture("basic.xml");
        let edited = applied(&bytes, set("HeadtrackingToggle", "0"));
        let expected = text(&bytes).replace(
            "<Attr name=\"HeadtrackingToggle\" value=\"1\"/>",
            "<Attr name=\"HeadtrackingToggle\" value=\"0\"/>",
        );
        assert_eq!(text(&edited), expected);

        let bytes = fixture("lf_tabs.xml");
        let edited = applied(&bytes, set("HeadtrackingToggle", "0.75"));
        let expected = text(&bytes).replace(
            "name=\"HeadtrackingToggle\" value=\"1\" origin=\"menu\"",
            "name=\"HeadtrackingToggle\" value=\"0.75\" origin=\"menu\"",
        );
        assert_eq!(text(&edited), expected);
    }

    #[test]
    fn set_new_appends_as_last_child_matching_neighbours() {
        let bytes = fixture("basic.xml");
        let mut doc = parse(&bytes).unwrap();
        doc.apply(&set("NewOne", "5")).unwrap();
        let expected = text(&bytes).replace(
            "</Attributes>",
            " <Attr name=\"NewOne\" value=\"5\"/>\r\n</Attributes>",
        );
        assert_eq!(text(&doc.to_bytes()), expected);
        assert_eq!(doc.entries().last().unwrap().name, "NewOne");
        assert_eq!(doc.entries().len(), 13);

        let bytes = fixture("lf_tabs.xml");
        let expected = text(&bytes).replace(
            "</Attributes>",
            "\t<Attr name=\"NewOne\" value=\"5\"/>\n</Attributes>",
        );
        assert_eq!(text(&applied(&bytes, set("NewOne", "5"))), expected);

        let no_final_newline =
            b"<Attributes Version=\"1\">\n <Attr name=\"A\" value=\"1\"/>\n</Attributes>";
        assert_eq!(
            text(&applied(no_final_newline, set("B", "2"))),
            "<Attributes Version=\"1\">\n <Attr name=\"A\" value=\"1\"/>\n <Attr name=\"B\" value=\"2\"/>\n</Attributes>"
        );
    }

    #[test]
    fn set_on_empty_document_creates_the_file() {
        assert_eq!(
            applied(b"", set("X", "1")),
            b"<Attributes Version=\"1\">\r\n <Attr name=\"X\" value=\"1\"/>\r\n</Attributes>\r\n"
        );
    }

    #[test]
    fn set_on_an_empty_root_opens_it() {
        assert_eq!(
            text(&applied(b"<Attributes Version=\"1\"/>\r\n", set("X", "1"))),
            "<Attributes Version=\"1\">\r\n <Attr name=\"X\" value=\"1\"/>\r\n</Attributes>\r\n"
        );
    }

    #[test]
    fn set_value_needing_escapes_round_trips_through_get() {
        let tricky = "a&b\"c<d>'e\tf\r\ng";
        for name in ["basic.xml", "single_quotes.xml"] {
            for target in ["FOV", "Callsign", "BrandNew"] {
                let mut doc = parse(&fixture(name)).unwrap();
                doc.apply(&set(target, tricky)).unwrap();
                assert_eq!(doc.get(target), Some(tricky), "{name} {target}");
                let reparsed = parse(&doc.to_bytes()).unwrap();
                assert_eq!(reparsed.get(target), Some(tricky), "{name} {target}");
                assert_eq!(reparsed, doc);
            }
        }
    }

    #[test]
    fn remove_deletes_the_line_and_keeps_comments() {
        let bytes = fixture("lf_tabs.xml");
        let edited = applied(&bytes, remove("HeadtrackingSource"));
        let expected =
            text(&bytes).replace("\t<Attr name=\"HeadtrackingSource\" value=\"1\"/>\n", "");
        assert_eq!(text(&edited), expected);
        assert!(text(&edited).contains("\t<!-- head tracking -->\n"));

        let edited = applied(&bytes, remove("HeadtrackingToggle"));
        let expected = text(&bytes).replace(
            "\t<Attr name=\"HeadtrackingToggle\" value=\"1\" origin=\"menu\"/>\n",
            "",
        );
        assert_eq!(text(&edited), expected);

        let mut doc = parse(&fixture("basic.xml")).unwrap();
        doc.apply(&remove("FOV")).unwrap();
        assert_eq!(doc.entries().len(), 11);
        assert_eq!(doc.get("FOV"), None);
    }

    #[test]
    fn remove_absent_is_invalid() {
        let mut doc = parse(&fixture("basic.xml")).unwrap();
        let before = doc.clone();
        assert!(matches!(
            doc.apply(&remove("Missing")),
            Err(Error::Invalid(_))
        ));
        assert_eq!(doc, before);

        let mut empty = parse(b"").unwrap();
        assert!(matches!(empty.apply(&remove("X")), Err(Error::Invalid(_))));
    }

    #[test]
    fn set_rejects_bad_names() {
        let mut doc = parse(&fixture("basic.xml")).unwrap();
        let before = doc.clone();
        for bad in [
            "", "a b", "a\"b", "a'b", "a<b", "a>b", "a&b", "a\tb", "a\nb",
        ] {
            assert!(
                matches!(doc.apply(&set(bad, "1")), Err(Error::Invalid(_))),
                "{bad:?}"
            );
            assert!(
                matches!(doc.apply(&remove(bad)), Err(Error::Invalid(_))),
                "{bad:?}"
            );
        }
        assert!(matches!(
            doc.apply(&set("FOV", "a\0b")),
            Err(Error::Invalid(_))
        ));
        assert_eq!(doc, before);
    }

    #[test]
    fn nested_duplicate_wrong_root_doctype_and_text_child_are_not_recognised() {
        let fixtures = [
            ("nested.xml", 3),
            ("duplicate.xml", 4),
            ("wrong_root.xml", 1),
            ("doctype.xml", 1),
            ("child_with_text.xml", 3),
        ];
        let inline: [(&[u8], usize); 7] = [
            (
                b"<Attributes>\n <Attr name=\"A\" value=\"1\"/>\n stray\n</Attributes>\n",
                3,
            ),
            (b"<Attributes>\n <Attr name=\"A\"/>\n</Attributes>\n", 2),
            (b"<Attributes>\n <Attr value=\"1\"/>\n</Attributes>\n", 2),
            (
                b"<Attributes>\n <Other name=\"A\" value=\"1\"/>\n</Attributes>\n",
                2,
            ),
            (
                b"<Attributes>\n <Attr name=\"A\" value=\"1\"></Attr>\n</Attributes>\n",
                2,
            ),
            (b"<Attributes/>\n<Attributes/>\n", 2),
            (b"\n\n", 1),
        ];
        let cases = fixtures
            .iter()
            .map(|(name, line)| (name.to_string(), fixture(name), *line))
            .chain(
                inline
                    .iter()
                    .map(|(bytes, line)| (text(bytes), bytes.to_vec(), *line)),
            );
        for (label, bytes, expected_line) in cases {
            let (line, message) = not_recognised(&bytes);
            assert!(
                message.starts_with("format not recognised: "),
                "{label}: {message}"
            );
            assert!(message.len() > "format not recognised: ".len(), "{label}");
            assert_eq!(line, expected_line, "{label}: {message}");
        }
    }

    #[test]
    fn malformed_is_parse_error() {
        let (line, message) = not_recognised(&fixture("unclosed.xml"));
        assert!(!message.starts_with("format not recognised"), "{message}");
        assert_eq!(line, 1);

        let (line, _) =
            not_recognised(b"<Attributes>\n <Attr name=\"A\" value=\"1/>\n</Attributes>\n");
        assert_eq!(line, 2);

        let (line, message) =
            not_recognised(b"<Attributes>\n <Attr name=\"\xff\" value=\"1\"/>\n</Attributes>\n");
        assert_eq!(line, 2);
        assert!(message.contains("UTF-8"), "{message}");
    }

    #[test]
    fn bom_is_preserved_through_an_edit() {
        let bytes = fixture("bom.xml");
        assert!(bytes.starts_with(BOM));
        let doc = parse(&bytes).unwrap();
        assert_eq!(doc.entries()[0].name, "HeadtrackingToggle");

        let edited = applied(&bytes, set("FOV", "100"));
        assert!(edited.starts_with(BOM));
        assert_eq!(
            text(&edited),
            text(&bytes).replace("value=\"90\"", "value=\"100\"")
        );

        let edited = applied(&bytes, set("New", "1"));
        assert!(edited.starts_with(BOM));
        assert_eq!(
            text(&edited),
            text(&bytes).replace(
                "</Attributes>",
                " <Attr name=\"New\" value=\"1\"/>\r\n</Attributes>"
            )
        );

        let created = applied(BOM, set("X", "1"));
        assert!(created.starts_with(BOM));
        assert_eq!(
            &created[3..],
            b"<Attributes Version=\"1\">\r\n <Attr name=\"X\" value=\"1\"/>\r\n</Attributes>\r\n"
        );
    }
}
