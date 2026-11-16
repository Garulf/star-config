//! Span-based edits of XML text that leave untouched bytes as they were.

use crate::error::{Error, Result};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::fmt::Write as _;
use std::ops::Range;
use std::path::Path;

const BOM: &str = "\u{feff}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub depth: usize,
    pub parent: Option<usize>,
    pub span: Range<usize>,
    pub open_tag: Range<usize>,
    pub close_tag: Option<Range<usize>>,
    pub attrs: Vec<Attr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub name: String,
    pub value: String,
    pub value_span: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct Scan {
    pub elements: Vec<Element>,
    pub has_doctype: bool,
}

pub fn scan(path: &Path, text: &str) -> Result<Scan> {
    let offset = if text.starts_with(BOM) { BOM.len() } else { 0 };
    let mut reader = Reader::from_str(&text[offset..]);
    let config = reader.config_mut();
    config.trim_text(false);
    config.expand_empty_elements = false;
    config.check_end_names = true;

    let mut elements: Vec<Element> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut has_doctype = false;
    loop {
        let start = offset + reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|err| {
            parse_error(
                path,
                text,
                offset + reader.error_position() as usize,
                err.to_string(),
            )
        })?;
        let end = offset + reader.buffer_position() as usize;
        match event {
            Event::Empty(tag) => {
                elements.push(element(path, text, &tag, start..end, &open)?);
            }
            Event::Start(tag) => {
                elements.push(element(path, text, &tag, start..end, &open)?);
                open.push(elements.len() - 1);
            }
            Event::End(_) => {
                let Some(index) = open.pop() else {
                    return Err(parse_error(
                        path,
                        text,
                        start,
                        "unexpected closing tag".into(),
                    ));
                };
                let element = &mut elements[index];
                element.span = element.open_tag.start..end;
                element.close_tag = Some(start..end);
            }
            Event::DocType(_) => has_doctype = true,
            Event::Eof => break,
            _ => {}
        }
    }
    if let Some(&index) = open.last() {
        let element = &elements[index];
        return Err(parse_error(
            path,
            text,
            element.open_tag.start,
            format!("<{}> is never closed", element.name),
        ));
    }
    Ok(Scan {
        elements,
        has_doctype,
    })
}

fn element(
    path: &Path,
    text: &str,
    tag: &BytesStart,
    open_tag: Range<usize>,
    open: &[usize],
) -> Result<Element> {
    let fail = |message: String| parse_error(path, text, open_tag.start, message);
    for attribute in tag.attributes() {
        attribute.map_err(|err| fail(err.to_string()))?;
    }
    let name = tag.name().into_inner().to_string();
    let attrs = attributes(&text[open_tag.clone()], 1 + name.len())
        .map_err(fail)?
        .into_iter()
        .map(|(name, value_span)| {
            let raw = &text[open_tag.start + value_span.start..open_tag.start + value_span.end];
            let value = quick_xml::escape::unescape(raw)
                .map_err(|err| fail(format!("attribute {name}: {err}")))?
                .into_owned();
            Ok(Attr {
                name,
                value,
                value_span: open_tag.start + value_span.start..open_tag.start + value_span.end,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Element {
        name,
        depth: open.len(),
        parent: open.last().copied(),
        span: open_tag.clone(),
        open_tag,
        close_tag: None,
        attrs,
    })
}

/// Returns each attribute's name and the span of its value relative to `tag`, quotes excluded.
fn attributes(
    tag: &str,
    mut at: usize,
) -> std::result::Result<Vec<(String, Range<usize>)>, String> {
    let bytes = tag.as_bytes();
    let skip_space = |mut at: usize| {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        at
    };
    let mut found: Vec<(String, Range<usize>)> = Vec::new();
    loop {
        at = skip_space(at);
        if at >= bytes.len() || matches!(bytes[at], b'/' | b'>') {
            return Ok(found);
        }
        let name_start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() && !b"=/>".contains(&bytes[at]) {
            at += 1;
        }
        let name = tag[name_start..at].to_string();
        at = skip_space(at);
        if bytes.get(at) != Some(&b'=') {
            return Err(format!("attribute {name} has no value"));
        }
        at = skip_space(at + 1);
        let quote = match bytes.get(at) {
            Some(&quote @ (b'"' | b'\'')) => quote,
            _ => return Err(format!("value of attribute {name} is not quoted")),
        };
        let value_start = at + 1;
        let Some(length) = bytes[value_start..].iter().position(|&b| b == quote) else {
            return Err(format!("value of attribute {name} is not closed"));
        };
        if found.iter().any(|(seen, _)| *seen == name) {
            return Err(format!("attribute {name} appears twice"));
        }
        found.push((name, value_start..value_start + length));
        at = value_start + length + 1;
    }
}

fn parse_error(path: &Path, text: &str, at: usize, message: String) -> Error {
    Error::Parse {
        path: path.to_path_buf(),
        line: line_at(text, at),
        message,
    }
}

pub(crate) fn line_at(text: &str, at: usize) -> usize {
    let end = at.min(text.len());
    text.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

pub fn escape_attr(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            c if c.is_control() => {
                let _ = write!(escaped, "&#x{:X};", u32::from(c));
            }
            c => escaped.push(c),
        }
    }
    escaped
}

pub fn set_attr(text: &mut String, attr: &Attr, new_value: &str) {
    let single_quoted = attr
        .value_span
        .start
        .checked_sub(1)
        .is_some_and(|quote| text.as_bytes()[quote] == b'\'');
    let mut escaped = escape_attr(new_value);
    if single_quoted {
        escaped = escaped.replace('\'', "&apos;");
    }
    text.replace_range(attr.value_span.clone(), &escaped);
}

/// Removes the element and, when it sits alone on its line, the whole line.
pub fn remove_element(text: &mut String, element: &Element) {
    let range = whole_line(text, &element.span).unwrap_or_else(|| element.span.clone());
    text.replace_range(range, "");
}

fn whole_line(text: &str, span: &Range<usize>) -> Option<Range<usize>> {
    let start = line_start(text, span.start);
    if !is_blank(&text[start..span.start]) {
        return None;
    }
    match text[span.end..].find('\n') {
        Some(newline) => {
            let end = span.end + newline + 1;
            is_blank(&text[span.end..end]).then_some(start..end)
        }
        None if is_blank(&text[span.end..]) => {
            let before = text[..start]
                .strip_suffix('\n')
                .map_or(start, |rest| rest.strip_suffix('\r').unwrap_or(rest).len());
            Some(before..text.len())
        }
        None => None,
    }
}

/// Inserts `xml` as the last child of `parent`, on its own line, indented like the parent's last child
/// (or one level deeper than the parent when it has none). An empty-element parent is opened up first.
pub fn append_child(text: &mut String, parent: &Element, xml: &str) {
    let newline = line_ending(text);
    let parent_indent = indent_of_line(text, parent.open_tag.start).to_string();
    let deeper = || format!("{parent_indent}{}", indent_unit(text));
    match &parent.close_tag {
        None => {
            let slash = parent.open_tag.start
                + text[parent.open_tag.clone()]
                    .rfind('/')
                    .unwrap_or(parent.open_tag.len() - 1);
            let opened = format!(
                ">{newline}{indent}{xml}{newline}{parent_indent}</{name}>",
                indent = deeper(),
                name = parent.name
            );
            text.replace_range(slash..parent.open_tag.end, &opened);
        }
        Some(close) => {
            let indent = match last_child_start(text, parent.open_tag.end..close.start) {
                Some(child) => indent_of_line(text, child).to_string(),
                None => deeper(),
            };
            let close_line = line_start(text, close.start);
            if close_line >= parent.open_tag.end && is_blank(&text[close_line..close.start]) {
                text.insert_str(close_line, &format!("{indent}{xml}{newline}"));
            } else {
                text.insert_str(
                    close.start,
                    &format!("{newline}{indent}{xml}{newline}{parent_indent}"),
                );
            }
        }
    }
}

fn last_child_start(text: &str, content: Range<usize>) -> Option<usize> {
    let mut reader = Reader::from_str(&text[content.clone()]);
    reader.config_mut().check_end_names = false;
    let mut depth = 0usize;
    let mut last = None;
    loop {
        let start = content.start + reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(_)) => {
                if depth == 0 {
                    last = Some(start);
                }
                depth += 1;
            }
            Ok(Event::Empty(_)) if depth == 0 => last = Some(start),
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::Eof) | Err(_) => return last,
            Ok(_) => {}
        }
    }
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |newline| newline + 1)
}

fn indent_of_line(text: &str, at: usize) -> &str {
    let line = &text[line_start(text, at)..at];
    let width = line
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    &line[..width]
}

fn line_ending(text: &str) -> &'static str {
    match text.find('\n') {
        Some(newline) if text[..newline].ends_with('\r') => "\r\n",
        Some(_) => "\n",
        None => "\r\n",
    }
}

fn indent_unit(text: &str) -> String {
    let mut fewest_spaces: Option<usize> = None;
    for line in text.lines() {
        let indent = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
        if indent.is_empty() || !line[indent.len()..].starts_with('<') {
            continue;
        }
        if indent.starts_with('\t') {
            return "\t".into();
        }
        let spaces = indent.bytes().take_while(|&b| b == b' ').count();
        fewest_spaces = Some(fewest_spaces.map_or(spaces, |fewest| fewest.min(spaces)));
    }
    " ".repeat(fewest_spaces.unwrap_or(1))
}

fn is_blank(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use std::path::Path;

    fn scan_ok(text: &str) -> Scan {
        scan(Path::new("t.xml"), text).unwrap()
    }

    fn slice_of(text: &str, needle: &str) -> std::ops::Range<usize> {
        let start = text.find(needle).unwrap();
        start..start + needle.len()
    }

    fn parse_line(text: &str) -> usize {
        match scan(Path::new("t.xml"), text).unwrap_err() {
            Error::Parse { line, path, .. } => {
                assert_eq!(path, Path::new("t.xml"));
                line
            }
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    #[test]
    fn scan_reports_names_depths_parents_and_spans() {
        let text = "<?xml version=\"1.0\"?>\r\n<Root a=\"1\">\r\n <!-- <X/> -->\r\n <A x=\"2\"/>\r\n <B>\r\n  <C/>\r\n </B >\r\n</Root>\r\n";
        let found = scan_ok(text);
        let shape: Vec<_> = found
            .elements
            .iter()
            .map(|e| (e.name.as_str(), e.depth, e.parent))
            .collect();
        assert_eq!(
            shape,
            [
                ("Root", 0, None),
                ("A", 1, Some(0)),
                ("B", 1, Some(0)),
                ("C", 2, Some(2))
            ]
        );
        assert!(!found.has_doctype);

        let root = &found.elements[0];
        let root_start = text.find("<Root").unwrap();
        let root_end = text.find("</Root>").unwrap() + "</Root>".len();
        assert_eq!(root.span, root_start..root_end);
        assert_eq!(root.open_tag, slice_of(text, "<Root a=\"1\">"));
        assert_eq!(root.close_tag, Some(slice_of(text, "</Root>")));

        let a = &found.elements[1];
        assert_eq!(&text[a.span.clone()], "<A x=\"2\"/>");
        assert_eq!(a.open_tag, a.span);
        assert_eq!(a.close_tag, None);

        let b = &found.elements[2];
        assert_eq!(&text[b.open_tag.clone()], "<B>");
        assert_eq!(&text[b.close_tag.clone().unwrap()], "</B >");
        assert_eq!(&text[b.span.clone()], "<B>\r\n  <C/>\r\n </B >");

        assert_eq!(&text[found.elements[3].span.clone()], "<C/>");
    }

    #[test]
    fn scan_unescapes_values_and_spans_exclude_quotes() {
        let text = "<R v=\"a &amp; b &quot;c&quot; &lt;d&gt; &#65;&#x42;\" w = \"plain\"/>";
        let found = scan_ok(text);
        let attrs = &found.elements[0].attrs;
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].name, "v");
        assert_eq!(attrs[0].value, "a & b \"c\" <d> AB");
        assert_eq!(
            &text[attrs[0].value_span.clone()],
            "a &amp; b &quot;c&quot; &lt;d&gt; &#65;&#x42;"
        );
        assert_eq!(attrs[1].name, "w");
        assert_eq!(attrs[1].value, "plain");
        assert_eq!(&text[attrs[1].value_span.clone()], "plain");
    }

    #[test]
    fn scan_handles_single_quoted_attributes() {
        let text = "<R a='it&apos;s \"q\"' b=\"x\" c=''/>";
        let attrs = &scan_ok(text).elements[0].attrs;
        assert_eq!(attrs[0].value, "it's \"q\"");
        assert_eq!(&text[attrs[0].value_span.clone()], "it&apos;s \"q\"");
        assert_eq!(attrs[1].value, "x");
        assert_eq!(attrs[2].value, "");
        assert!(attrs[2].value_span.is_empty());
        assert_eq!(text.as_bytes()[attrs[2].value_span.start - 1], b'\'');
    }

    #[test]
    fn scan_flags_doctype() {
        assert!(scan_ok("<!DOCTYPE R>\n<R/>\n").has_doctype);
        assert!(!scan_ok("<R/>\n").has_doctype);
    }

    #[test]
    fn scan_skips_a_byte_order_mark_and_keeps_offsets() {
        let text = "\u{feff}<R a=\"1\"/>";
        let found = scan_ok(text);
        assert_eq!(&text[found.elements[0].span.clone()], "<R a=\"1\"/>");
        assert_eq!(&text[found.elements[0].attrs[0].value_span.clone()], "1");
    }

    #[test]
    fn scan_malformed_is_parse_error_with_line() {
        assert_eq!(parse_line("<R>\n <A>\n</R>\n"), 3);
        assert_eq!(parse_line("<R>\n<A/>\n"), 1);
        assert_eq!(parse_line("<R>\n<A v=\"1/>\n</R>\n"), 2);
        assert_eq!(parse_line("<R>\n\n<A a=\"1\" a=\"2\"/>\n</R>\n"), 3);
        assert_eq!(parse_line("<R>\n<A a=1/>\n</R>\n"), 2);
        assert_eq!(parse_line("<R>\n<A a=\"&bogus;\"/>\n</R>\n"), 2);
        assert_eq!(parse_line("</R>"), 1);
    }

    #[test]
    fn escape_attr_covers_markup_quotes_and_controls() {
        assert_eq!(escape_attr("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e'f");
        assert_eq!(escape_attr("x\ty\nz\r"), "x&#x9;y&#xA;z&#xD;");
        assert_eq!(escape_attr("plain"), "plain");
    }

    #[test]
    fn set_attr_changes_only_the_value_bytes_and_escapes() {
        let original = "<R>\r\n <A v=\"old\" w=\"x\"/>\r\n</R>\r\n";
        let mut text = original.to_string();
        let attr = scan_ok(&text).elements[1].attrs[0].clone();
        set_attr(&mut text, &attr, "a&b\"c");
        assert_eq!(text, original.replace("v=\"old\"", "v=\"a&amp;b&quot;c\""));
        assert_eq!(scan_ok(&text).elements[1].attrs[0].value, "a&b\"c");

        let original = "<R v='x' w=\"y\"/>";
        let mut text = original.to_string();
        let attr = scan_ok(&text).elements[0].attrs[0].clone();
        set_attr(&mut text, &attr, "it's \"q\"");
        assert_eq!(text, "<R v='it&apos;s &quot;q&quot;' w=\"y\"/>");
        assert_eq!(scan_ok(&text).elements[0].attrs[0].value, "it's \"q\"");
    }

    #[test]
    fn remove_element_takes_its_whole_line() {
        let text = "<R>\r\n <A/>\r\n <B>\r\n  <C/>\r\n </B>\r\n <D/>\r\n</R>\r\n";
        let mut edited = text.to_string();
        let b = scan_ok(text).elements[2].clone();
        remove_element(&mut edited, &b);
        assert_eq!(edited, "<R>\r\n <A/>\r\n <D/>\r\n</R>\r\n");

        let text = "<R>\n\t<A/>\n\t<!-- keep -->\n\t<B/>\n</R>";
        let mut edited = text.to_string();
        let a = scan_ok(text).elements[1].clone();
        remove_element(&mut edited, &a);
        assert_eq!(edited, "<R>\n\t<!-- keep -->\n\t<B/>\n</R>");

        let text = "<R/>\r\n\t<!-- tail -->";
        let mut edited = text.to_string();
        let r = scan_ok(text).elements[0].clone();
        remove_element(&mut edited, &r);
        assert_eq!(edited, "\t<!-- tail -->");
    }

    #[test]
    fn remove_element_at_the_end_without_final_newline_takes_the_line_break_before() {
        let text = "<!-- c -->\r\n<R/>";
        let mut edited = text.to_string();
        let r = scan_ok(text).elements[0].clone();
        remove_element(&mut edited, &r);
        assert_eq!(edited, "<!-- c -->");
    }

    #[test]
    fn remove_element_inline_leaves_siblings() {
        let text = "<R>\n <A/><B/>\n</R>\n";
        let found = scan_ok(text);

        let mut edited = text.to_string();
        remove_element(&mut edited, &found.elements[1]);
        assert_eq!(edited, "<R>\n <B/>\n</R>\n");

        let mut edited = text.to_string();
        remove_element(&mut edited, &found.elements[2]);
        assert_eq!(edited, "<R>\n <A/>\n</R>\n");

        let text = "<R>\n <A/> <!-- note -->\n</R>\n";
        let mut edited = text.to_string();
        remove_element(&mut edited, &scan_ok(text).elements[1]);
        assert_eq!(edited, "<R>\n  <!-- note -->\n</R>\n");
    }

    #[test]
    fn append_child_copies_indent_and_line_ending() {
        let text = "<R>\r\n <A/>\r\n</R>\r\n";
        let mut edited = text.to_string();
        append_child(&mut edited, &scan_ok(text).elements[0], "<B/>");
        assert_eq!(edited, "<R>\r\n <A/>\r\n <B/>\r\n</R>\r\n");

        let text = "<R>\n\t<A>\n\t\t<X/>\n\t</A>\n\t<!-- c -->\n</R>\n";
        let mut edited = text.to_string();
        append_child(&mut edited, &scan_ok(text).elements[0], "<B/>");
        assert_eq!(
            edited,
            "<R>\n\t<A>\n\t\t<X/>\n\t</A>\n\t<!-- c -->\n\t<B/>\n</R>\n"
        );

        let text = "<R>\n\t<P>\n\t</P>\n</R>\n";
        let mut edited = text.to_string();
        append_child(&mut edited, &scan_ok(text).elements[1], "<B/>");
        assert_eq!(edited, "<R>\n\t<P>\n\t\t<B/>\n\t</P>\n</R>\n");

        let text = "<R>\r\n <A/></R>";
        let mut edited = text.to_string();
        append_child(&mut edited, &scan_ok(text).elements[0], "<B/>");
        assert_eq!(edited, "<R>\r\n <A/>\r\n <B/>\r\n</R>");
    }

    #[test]
    fn append_child_opens_an_empty_parent() {
        let text = "<A/>\n";
        let mut edited = text.to_string();
        append_child(&mut edited, &scan_ok(text).elements[0], "<B/>");
        assert_eq!(edited, "<A>\n <B/>\n</A>\n");

        let text = "<R>\r\n <A x=\"1\" />\r\n</R>\r\n";
        let mut edited = text.to_string();
        append_child(&mut edited, &scan_ok(text).elements[1], "<B/>");
        assert_eq!(
            edited,
            "<R>\r\n <A x=\"1\" >\r\n  <B/>\r\n </A>\r\n</R>\r\n"
        );
        assert_eq!(scan_ok(&edited).elements[2].parent, Some(1));
    }
}
