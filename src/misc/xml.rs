//! Minimal indenting XML writer producing output shaped like `XmlWriter` with `Indent = true`.

use std::fmt::Write as _;

/// An element tree node.
#[derive(Debug, Clone, Default)]
pub struct XElement {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<XElement>,
    pub text: Option<String>,
}

impl XElement {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: safe_name(&name.into()), ..Default::default() }
    }

    pub fn with_text(name: impl Into<String>, text: impl Into<String>) -> Self {
        let mut e = Self::new(name);
        e.text = Some(text.into());
        e
    }

    pub fn attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.push((key.into(), value.into()));
        self
    }

    pub fn set_attr(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.attributes.push((key.into(), value.into()));
    }

    pub fn add(&mut self, child: XElement) -> &mut Self {
        self.children.push(child);
        self
    }

    pub fn add_opt(&mut self, child: Option<XElement>) -> &mut Self {
        if let Some(c) = child {
            self.children.push(c);
        }
        self
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = Some(text.into());
    }

    /// Serialise with two-space indentation and `\n` line breaks, no XML declaration.
    pub fn to_string_indented(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, depth: usize) {
        let pad = "  ".repeat(depth);
        let _ = write!(out, "{pad}<{}", self.name);
        for (k, v) in &self.attributes {
            let _ = write!(out, " {}=\"{}\"", safe_name(k), escape_attr(v));
        }
        let text = self.text.as_deref().unwrap_or("");
        if self.children.is_empty() {
            if text.is_empty() {
                out.push_str(" />");
            } else {
                let _ = write!(out, ">{}</{}>", escape_text(text), self.name);
            }
        } else {
            out.push('>');
            if !text.is_empty() {
                out.push_str(&escape_text(text));
            }
            out.push('\n');
            for c in &self.children {
                c.write(out, depth + 1);
                out.push('\n');
            }
            let _ = write!(out, "{pad}</{}>", self.name);
        }
    }
}

/// Make an arbitrary string usable as an XML element/attribute name.
pub fn safe_name(raw: &str) -> String {
    if raw.trim().is_empty() {
        return "Empty".to_string();
    }
    let mut cleaned: String = raw
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect();
    let first = cleaned.chars().next().unwrap();
    if !(first.is_alphabetic() || first == '_') {
        cleaned.insert(0, '_');
    }
    cleaned
}

pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            c if is_xml_char(c) => out.push(c),
            c => {
                let _ = write!(out, "&#x{:X};", c as u32);
            }
        }
    }
    out
}

pub fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            '\t' => out.push_str("&#x9;"),
            c if is_xml_char(c) => out.push(c),
            c => {
                let _ = write!(out, "&#x{:X};", c as u32);
            }
        }
    }
    out
}

/// `XmlConvert.IsXmlChar` equivalent.
pub fn is_xml_char(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_nested() {
        let mut root = XElement::new("File");
        root.add(XElement::with_text("Path", "a<b").attr("t", "x\"y"));
        root.add(XElement::new("Empty"));
        let s = root.to_string_indented();
        assert_eq!(s, "<File>\n  <Path t=\"x&quot;y\">a&lt;b</Path>\n  <Empty />\n</File>");
    }

    #[test]
    fn sanitises_names() {
        assert_eq!(safe_name("Bits-(Pixel*Frame)"), "Bits-_Pixel_Frame_");
        assert_eq!(safe_name("1abc"), "_1abc");
        assert_eq!(safe_name("  "), "Empty");
    }
}
