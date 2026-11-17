//! Decoder for CryXmlB, the binary XML that CryEngine games store their
//! configuration in.

use crate::p4k::MAX_ENTRY_BYTES;
use std::borrow::Cow;

const MAGIC: &[u8] = b"CryXmlB";
const HEADER_LEN: usize = 44;
const NODE_LEN: usize = 28;
const ATTR_LEN: usize = 8;
const CHILD_LEN: usize = 4;
const MAX_XML_BYTES: usize = 2 * MAX_ENTRY_BYTES as usize;

pub fn is_cryxml(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

pub fn to_xml(bytes: &[u8]) -> Option<String> {
    Document::parse(bytes)?.render()
}

pub fn decode_entry(bytes: Vec<u8>) -> Result<String, Vec<u8>> {
    if is_cryxml(&bytes) {
        return to_xml(&bytes).ok_or(bytes);
    }
    match String::from_utf8(bytes) {
        Ok(text) => {
            let body = text.strip_prefix('\u{feff}').unwrap_or(&text);
            if body.trim_start().starts_with('<') {
                Ok(body.to_string())
            } else {
                Err(text.into_bytes())
            }
        }
        Err(err) => Err(err.into_bytes()),
    }
}

struct Table {
    start: usize,
    count: usize,
}

struct Document<'a> {
    bytes: &'a [u8],
    nodes: Table,
    attrs: Table,
    children: Table,
    strings: &'a [u8],
}

struct Node {
    tag: u32,
    content: u32,
    attr_count: usize,
    child_count: usize,
    first_attr: i32,
    first_child: i32,
}

enum Step {
    Open(usize),
    Close(u32),
}

impl<'a> Document<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < HEADER_LEN || !is_cryxml(bytes) {
            return None;
        }
        let field = |index: usize| u32_at(bytes, 8 + 4 * index);
        let table = |pos: u32, count: u32, width: usize| {
            let start = pos as usize;
            let length = (count as usize).checked_mul(width)?;
            (start.checked_add(length)? <= bytes.len()).then_some(Table {
                start,
                count: count as usize,
            })
        };
        let strings_start = field(7) as usize;
        let strings_end = strings_start.checked_add(field(8) as usize)?;
        Some(Document {
            bytes,
            nodes: table(field(1), field(2), NODE_LEN)?,
            attrs: table(field(3), field(4), ATTR_LEN)?,
            children: table(field(5), field(6), CHILD_LEN)?,
            strings: bytes.get(strings_start..strings_end)?,
        })
    }

    fn string(&self, offset: u32) -> Option<Cow<'a, str>> {
        let rest = self.strings.get(offset as usize..)?;
        let end = rest.iter().position(|&byte| byte == 0)?;
        Some(String::from_utf8_lossy(&rest[..end]))
    }

    fn node(&self, index: usize) -> Node {
        let at = self.nodes.start + index * NODE_LEN;
        Node {
            tag: u32_at(self.bytes, at),
            content: u32_at(self.bytes, at + 4),
            attr_count: usize::from(u16_at(self.bytes, at + 8)),
            child_count: usize::from(u16_at(self.bytes, at + 10)),
            first_attr: u32_at(self.bytes, at + 16) as i32,
            first_child: u32_at(self.bytes, at + 20) as i32,
        }
    }

    fn range(table: &Table, first: i32, count: usize) -> Option<std::ops::Range<usize>> {
        if count == 0 {
            return Some(0..0);
        }
        let first = usize::try_from(first).ok()?;
        let end = first.checked_add(count)?;
        (end <= table.count).then_some(first..end)
    }

    fn render(&self) -> Option<String> {
        if self.nodes.count == 0 {
            return None;
        }
        let mut out = String::new();
        let mut seen = vec![false; self.nodes.count];
        seen[0] = true;
        let mut steps = vec![Step::Open(0)];
        while let Some(step) = steps.pop() {
            match step {
                Step::Close(tag) => {
                    out.push_str("</");
                    out.push_str(&self.string(tag)?);
                    out.push('>');
                }
                Step::Open(index) => self.open(index, &mut out, &mut seen, &mut steps)?,
            }
            if out.len() > MAX_XML_BYTES {
                return None;
            }
        }
        Some(out)
    }

    fn open(
        &self,
        index: usize,
        out: &mut String,
        seen: &mut [bool],
        steps: &mut Vec<Step>,
    ) -> Option<()> {
        let node = self.node(index);
        let tag = self.string(node.tag)?;
        let text = self.string(node.content)?;
        out.push('<');
        out.push_str(&tag);
        for attr in Self::range(&self.attrs, node.first_attr, node.attr_count)? {
            let at = self.attrs.start + attr * ATTR_LEN;
            out.push(' ');
            out.push_str(&self.string(u32_at(self.bytes, at))?);
            out.push_str("=\"");
            push_escaped(out, &self.string(u32_at(self.bytes, at + 4))?, true);
            out.push('"');
            within_cap(out)?;
        }
        let kids = Self::range(&self.children, node.first_child, node.child_count)?;
        if kids.is_empty() && text.is_empty() {
            out.push_str("/>");
            return Some(());
        }
        out.push('>');
        push_escaped(out, &text, false);
        within_cap(out)?;
        steps.push(Step::Close(node.tag));
        for kid in kids.rev() {
            let child = u32_at(self.bytes, self.children.start + kid * CHILD_LEN) as i32;
            let child = usize::try_from(child).ok().filter(|&c| c < seen.len())?;
            if std::mem::replace(&mut seen[child], true) {
                return None;
            }
            steps.push(Step::Open(child));
        }
        Some(())
    }
}

fn within_cap(out: &str) -> Option<()> {
    (out.len() <= MAX_XML_BYTES).then_some(())
}

fn push_escaped(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\r' => out.push_str("&#13;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\t' if attribute => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

#[cfg(test)]
pub(crate) fn encode(xml: &str) -> Vec<u8> {
    use quick_xml::events::{BytesStart, Event};
    use quick_xml::{Reader, XmlVersion};
    use std::collections::HashMap;

    struct Element {
        tag: String,
        attrs: Vec<(String, String)>,
        text: String,
        children: Vec<usize>,
        parent: i32,
    }

    fn add(elements: &mut Vec<Element>, open: &[usize], start: &BytesStart) -> usize {
        let attrs = start
            .attributes()
            .map(|attr| {
                let attr = attr.unwrap();
                let value = attr.normalized_value(XmlVersion::Implicit1_0).unwrap();
                (attr.key.as_ref().to_string(), value.into_owned())
            })
            .collect();
        elements.push(Element {
            tag: start.name().as_ref().to_string(),
            attrs,
            text: String::new(),
            children: Vec::new(),
            parent: open.last().map_or(-1, |&parent| parent as i32),
        });
        let id = elements.len() - 1;
        if let Some(&parent) = open.last() {
            elements[parent].children.push(id);
        }
        id
    }

    fn intern(strings: &mut Vec<u8>, known: &mut HashMap<String, u32>, text: &str) -> u32 {
        *known.entry(text.to_string()).or_insert_with(|| {
            let at = strings.len() as u32;
            strings.extend(text.as_bytes());
            strings.push(0);
            at
        })
    }

    let mut elements: Vec<Element> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event().unwrap() {
            Event::Start(start) => {
                let id = add(&mut elements, &open, &start);
                open.push(id);
            }
            Event::Empty(start) => {
                add(&mut elements, &open, &start);
            }
            Event::End(_) => {
                open.pop();
            }
            Event::Text(text) => {
                let text = text.xml_content(XmlVersion::Implicit1_0);
                if let (Some(&id), false) = (open.last(), text.trim().is_empty()) {
                    elements[id].text.push_str(&text);
                }
            }
            Event::GeneralRef(_) => panic!("encode does not take entity references in text"),
            Event::Eof => break,
            _ => {}
        }
    }

    let mut strings = Vec::new();
    let mut known = HashMap::new();
    let (mut nodes, mut attrs, mut children) = (Vec::new(), Vec::new(), Vec::new());
    for element in &elements {
        let tag = intern(&mut strings, &mut known, &element.tag);
        let content = intern(&mut strings, &mut known, &element.text);
        let first_attr = (attrs.len() / 2) as u32;
        let first_child = children.len() as u32;
        for (key, value) in &element.attrs {
            attrs.push(intern(&mut strings, &mut known, key));
            attrs.push(intern(&mut strings, &mut known, value));
        }
        children.extend(element.children.iter().map(|&child| child as u32));
        let counts = element.attrs.len() as u32 | ((element.children.len() as u32) << 16);
        nodes.extend([
            tag,
            content,
            counts,
            element.parent as u32,
            first_attr,
            first_child,
            0,
        ]);
    }

    let node_pos = HEADER_LEN;
    let attr_pos = node_pos + nodes.len() * 4;
    let child_pos = attr_pos + attrs.len() * 4;
    let string_pos = child_pos + children.len() * 4;
    let header = [
        (string_pos + strings.len()) as u32,
        node_pos as u32,
        elements.len() as u32,
        attr_pos as u32,
        (attrs.len() / 2) as u32,
        child_pos as u32,
        children.len() as u32,
        string_pos as u32,
        strings.len() as u32,
    ];
    let mut out = b"CryXmlB\0".to_vec();
    for word in header.into_iter().chain(nodes).chain(attrs).chain(children) {
        out.extend(word.to_le_bytes());
    }
    out.extend(strings);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::events::Event;
    use quick_xml::{Reader, XmlVersion};

    fn tree(xml: &str) -> Vec<String> {
        let describe = |start: &quick_xml::events::BytesStart| {
            let attrs: Vec<String> = start
                .attributes()
                .map(|attr| {
                    let attr = attr.unwrap();
                    let value = attr.normalized_value(XmlVersion::Implicit1_0).unwrap();
                    format!("{}={value}", attr.key.as_ref())
                })
                .collect();
            format!("{} [{}]", start.name().as_ref(), attrs.join(", "))
        };
        let mut events = Vec::new();
        let mut reader = Reader::from_str(xml);
        loop {
            match reader.read_event().unwrap() {
                Event::Start(start) => events.push(format!("open {}", describe(&start))),
                Event::Empty(start) => events.push(format!("empty {}", describe(&start))),
                Event::End(end) => events.push(format!("close {}", end.name().as_ref())),
                Event::Text(text) => events.push(format!(
                    "text {}",
                    text.xml_content(XmlVersion::Implicit1_0)
                )),
                Event::Eof => return events,
                _ => {}
            }
        }
    }

    fn header(fields: [u32; 9]) -> Vec<u8> {
        let mut bytes = b"CryXmlB\0".to_vec();
        for field in fields {
            bytes.extend(field.to_le_bytes());
        }
        bytes
    }

    fn header_field(bytes: &[u8], index: usize) -> usize {
        u32_at(bytes, 8 + 4 * index) as usize
    }

    fn set_child(bytes: &mut [u8], slot: usize, value: i32) {
        let at = header_field(bytes, 5) + 4 * slot;
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn cryxml_round_trips_attributes_children_and_text() {
        let xml = r#"<a x="1" y="&amp;"><b/><c>t</c></a>"#;
        let decoded = to_xml(&encode(xml)).unwrap();
        assert_eq!(tree(&decoded), tree(xml));
    }

    #[test]
    fn nested_and_repeated_elements_keep_their_order() {
        let xml = r#"<r><m n="1"><a k="v"/><a k="w"><i/></a></m><m n="2"/></r>"#;
        assert_eq!(to_xml(&encode(xml)).unwrap(), xml);
    }

    #[test]
    fn markup_characters_are_escaped_in_values_and_text() {
        let decoded = to_xml(&encode(r#"<a v="&lt;&quot;&#10;&amp;">x > y</a>"#)).unwrap();
        assert_eq!(decoded, r#"<a v="&lt;&quot;&#10;&amp;">x &gt; y</a>"#);
    }

    #[test]
    fn is_cryxml_looks_at_the_magic_only() {
        assert!(is_cryxml(&encode("<a/>")));
        assert!(!is_cryxml(b"<a/>"));
        assert!(!is_cryxml(b""));
    }

    #[test]
    fn decode_entry_passes_text_xml_and_rejects_binary() {
        assert_eq!(decode_entry(b"<a/>".to_vec()), Ok("<a/>".to_string()));
        assert_eq!(decode_entry(vec![0xff, 0, 1]), Err(vec![0xff, 0, 1]));
    }

    #[test]
    fn decode_entry_handles_cryxml_bom_and_prose() {
        assert_eq!(
            decode_entry(encode("<a x=\"1\"/>")),
            Ok("<a x=\"1\"/>".to_string())
        );
        assert_eq!(
            decode_entry("\u{feff}<a/>".as_bytes().to_vec()),
            Ok("<a/>".to_string())
        );
        assert_eq!(
            decode_entry(b"key=value".to_vec()),
            Err(b"key=value".to_vec())
        );
        let broken = b"CryXmlB\0 short".to_vec();
        assert_eq!(decode_entry(broken.clone()), Err(broken));
    }

    #[test]
    fn cryxml_with_out_of_range_offsets_is_none() {
        let tables: [[u32; 9]; 5] = [
            [44, 44, 1000, 0, 0, 0, 0, 44, 0],
            [44, 44, 0, 44, 1000, 0, 0, 44, 0],
            [44, 44, 0, 44, 0, 44, 1000, 44, 0],
            [44, 44, 0, 44, 0, 44, 0, 44, 10],
            [44, u32::MAX, 2, 0, 0, 0, 0, 44, 0],
        ];
        for fields in tables {
            assert_eq!(to_xml(&header(fields)), None, "{fields:?}");
        }
    }

    #[test]
    fn leaf_nodes_ignore_their_unused_first_indexes() {
        let mut bytes = encode("<a><b/></a>");
        let leaf = header_field(&bytes, 1) + NODE_LEN;
        for offset in [16, 20] {
            bytes[leaf + offset..leaf + offset + 4].copy_from_slice(&(-1i32).to_le_bytes());
        }
        assert_eq!(to_xml(&bytes).as_deref(), Some("<a><b/></a>"));
    }

    #[test]
    fn many_attributes_on_one_node_stop_at_the_output_cap() {
        let attr_count = u16::MAX as usize;
        let string = vec![b'x'; 4096];
        let attr_pos = HEADER_LEN + NODE_LEN;
        let string_pos = attr_pos + attr_count * ATTR_LEN;
        let mut bytes = header([
            (string_pos + string.len() + 1) as u32,
            HEADER_LEN as u32,
            1,
            attr_pos as u32,
            attr_count as u32,
            string_pos as u32,
            0,
            string_pos as u32,
            string.len() as u32 + 1,
        ]);
        let mut node = [0u8; NODE_LEN];
        node[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        bytes.extend(node);
        bytes.resize(string_pos, 0);
        bytes.extend(&string);
        bytes.push(0);
        assert_eq!(to_xml(&bytes), None);
    }

    #[test]
    fn a_document_without_nodes_is_none() {
        assert_eq!(to_xml(&header([44, 44, 0, 44, 0, 44, 0, 44, 0])), None);
    }

    #[test]
    fn child_links_that_loop_repeat_or_leave_the_table_are_none() {
        for child in [0, 1, 99, -1] {
            let mut bytes = encode("<a><b/><c/></a>");
            set_child(&mut bytes, 1, child);
            assert_eq!(to_xml(&bytes), None, "child {child}");
        }
    }

    #[test]
    fn string_offsets_past_the_table_or_without_a_terminator_are_none() {
        let mut bytes = encode("<a/>");
        let node = header_field(&bytes, 1);
        bytes[node..node + 4].copy_from_slice(&9999u32.to_le_bytes());
        assert_eq!(to_xml(&bytes), None);

        let mut bytes = encode("<a/>");
        let last = bytes.len() - 1;
        bytes[last] = b'x';
        assert_eq!(to_xml(&bytes), None);
    }

    #[test]
    fn every_prefix_of_a_document_is_handled_without_panicking() {
        let bytes = encode(r#"<a x="1"><b y="2">t</b><c/></a>"#);
        for len in 0..bytes.len() {
            let _ = to_xml(&bytes[..len]);
        }
        assert!(to_xml(&bytes).is_some());
    }

    #[test]
    fn deep_nesting_does_not_overflow_the_stack() {
        let depth = 20_000;
        let xml = format!("{}{}", "<a>".repeat(depth), "</a>".repeat(depth));
        let decoded = to_xml(&encode(&xml)).unwrap();
        assert_eq!(decoded.matches("<a>").count(), depth - 1);
    }
}
