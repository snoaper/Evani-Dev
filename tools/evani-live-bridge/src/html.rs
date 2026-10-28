//! A small HTML reader for the game's own page files (docs/LIVE_API.md 14):
//! enough to find the `<style>` blocks, stylesheet links, the title and the
//! elements with an `id`, with their byte spans in the source. It is not a
//! browser parser: it reads well-formed pages the way they are written
//! (no optional end tags are inferred), which is what the page hot-replace
//! needs, since both the old and the new text are read the same way.

/// Elements with no content and no end tag.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr",
];
/// Elements whose content is raw text up to their end tag.
const RAW: &[&str] = &["script", "style", "title", "textarea"];

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
    Comment,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// Lower case.
    pub tag: String,
    /// Names lower case, in source order; `None` for a bare attribute.
    pub attrs: Vec<(String, Option<String>)>,
    pub children: Vec<Node>,
    /// Byte offset of the `<` of the start tag.
    pub start: usize,
    /// Just after the start tag's `>`.
    pub open_end: usize,
    /// Where the content ends (the end tag's `<`, or `open_end` for a void element).
    pub close_start: usize,
    /// Just after the end tag (or the start tag for a void element).
    pub end: usize,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_deref().unwrap_or(""))
    }

    pub fn has_attr(&self, name: &str) -> bool {
        self.attrs.iter().any(|(n, _)| n == name)
    }

    /// The content's source text.
    pub fn inner<'a>(&self, src: &'a str) -> &'a str {
        &src[self.open_end..self.close_start]
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    /// Does any element below this one carry an `id`?
    pub fn has_id_below(&self) -> bool {
        self.elements().any(|e| e.has_attr("id") || e.has_id_below())
    }

    /// Every element in this subtree (this one first), depth first.
    pub fn walk<'a>(&'a self, out: &mut Vec<&'a Element>) {
        out.push(self);
        for e in self.elements() {
            e.walk(out);
        }
    }
}

/// A parsed document: a synthetic root element holding the top-level nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    pub root: Element,
}

impl Doc {
    /// Every element in document order.
    pub fn elements(&self) -> Vec<&Element> {
        let mut out = Vec::new();
        for e in self.root.elements() {
            e.walk(&mut out);
        }
        out
    }

    pub fn find(&self, pred: impl Fn(&Element) -> bool) -> Option<&Element> {
        self.elements().into_iter().find(|e| pred(e))
    }
}

fn starts_with_ci(s: &str, at: usize, pat: &str) -> bool {
    s.len() >= at + pat.len() && s.as_bytes()[at..at + pat.len()].eq_ignore_ascii_case(pat.as_bytes())
}

fn find_ci(s: &str, from: usize, pat: &str) -> Option<usize> {
    let hay = s.as_bytes();
    let needle = pat.as_bytes();
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.')
}

/// Attributes as read: lower-case names, `None` for a bare attribute.
type Attrs = Vec<(String, Option<String>)>;

/// A start tag at `at` (the `<`): name, attributes, self-closing, end.
fn start_tag(src: &str, at: usize) -> Result<(String, Attrs, bool, usize), String> {
    let b = src.as_bytes();
    let mut i = at + 1;
    let name_start = i;
    while i < b.len() && is_name_char(b[i]) {
        i += 1;
    }
    let tag = src[name_start..i].to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            return Err(format!("unclosed <{tag}> tag"));
        }
        match b[i] {
            b'>' => return Ok((tag, attrs, false, i + 1)),
            b'/' if b.get(i + 1) == Some(&b'>') => return Ok((tag, attrs, true, i + 2)),
            b'/' => {
                i += 1;
                continue;
            }
            _ => {}
        }
        let n0 = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let name = src[n0..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b.get(i) != Some(&b'=') {
            attrs.push((name, None));
            continue;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let value = match b.get(i) {
            Some(&q @ (b'"' | b'\'')) => {
                let v0 = i + 1;
                let Some(len) = src[v0..].find(q as char) else {
                    return Err(format!("unclosed attribute {name} in <{tag}>"));
                };
                i = v0 + len + 1;
                src[v0..v0 + len].to_string()
            }
            _ => {
                let v0 = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                src[v0..i].to_string()
            }
        };
        attrs.push((name, Some(value)));
    }
}

/// Read a page. Errors only on text that can't be a page (an unclosed tag,
/// comment or raw-text element).
pub fn parse(src: &str) -> Result<Doc, String> {
    let b = src.as_bytes();
    let new_el = |tag: &str, start: usize| Element {
        tag: tag.to_string(),
        attrs: Vec::new(),
        children: Vec::new(),
        start,
        open_end: start,
        close_start: start,
        end: start,
    };
    // The open elements; [0] is the synthetic root.
    let mut stack: Vec<Element> = vec![new_el("#root", 0)];
    let close_top = |stack: &mut Vec<Element>, close_start: usize, end: usize| {
        if let Some(mut e) = stack.pop() {
            e.close_start = close_start;
            e.end = end;
            if let Some(parent) = stack.last_mut() {
                parent.children.push(Node::Element(e));
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let next = src[i..].find('<').map_or(b.len(), |n| i + n);
            if let Some(top) = stack.last_mut() {
                top.children.push(Node::Text(src[i..next].to_string()));
            }
            i = next;
            continue;
        }
        if starts_with_ci(src, i, "<!--") {
            let end = src[i + 4..].find("-->").ok_or("unclosed comment")? + i + 7;
            if let Some(top) = stack.last_mut() {
                top.children.push(Node::Comment);
            }
            i = end;
        } else if starts_with_ci(src, i, "<!") || starts_with_ci(src, i, "<?") {
            i = src[i..].find('>').ok_or("unclosed <! tag")? + i + 1;
        } else if starts_with_ci(src, i, "</") {
            let n0 = i + 2;
            let mut j = n0;
            while j < b.len() && is_name_char(b[j]) {
                j += 1;
            }
            let name = src[n0..j].to_ascii_lowercase();
            let end = src[j..].find('>').ok_or("unclosed end tag")? + j + 1;
            // Close up to the matching open element; a stray end tag is ignored.
            if let Some(pos) = stack.iter().rposition(|e| e.tag == name)
                && pos > 0
            {
                while stack.len() > pos + 1 {
                    close_top(&mut stack, i, i);
                }
                close_top(&mut stack, i, end);
            }
            i = end;
        } else if b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
            let (tag, attrs, self_closing, open_end) = start_tag(src, i)?;
            let mut el = new_el(&tag, i);
            el.attrs = attrs;
            el.open_end = open_end;
            if VOID.contains(&tag.as_str()) || self_closing {
                el.close_start = open_end;
                el.end = open_end;
                if let Some(top) = stack.last_mut() {
                    top.children.push(Node::Element(el));
                }
                i = open_end;
            } else if RAW.contains(&tag.as_str()) {
                let close = find_ci(src, open_end, &format!("</{tag}")).ok_or_else(|| format!("unclosed <{tag}>"))?;
                let end = src[close..].find('>').ok_or("unclosed end tag")? + close + 1;
                el.children.push(Node::Text(src[open_end..close].to_string()));
                el.close_start = close;
                el.end = end;
                if let Some(top) = stack.last_mut() {
                    top.children.push(Node::Element(el));
                }
                i = end;
            } else {
                stack.push(el);
                i = open_end;
            }
        } else {
            // A lone '<' in text.
            let next = src[i + 1..].find('<').map_or(b.len(), |n| i + 1 + n);
            if let Some(top) = stack.last_mut() {
                top.children.push(Node::Text(src[i..next].to_string()));
            }
            i = next;
        }
    }
    while stack.len() > 1 {
        close_top(&mut stack, b.len(), b.len());
    }
    let mut root = stack.pop().ok_or("empty")?;
    root.close_start = b.len();
    root.end = b.len();
    Ok(Doc { root })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>Kart &amp; co</title>
<style>.a > .b { color: red; }</style>
<link data-trunk rel="inline" type="css" href="hud.css">
</head><body>
<!-- a <div> in a comment -->
<div id="ui" class='x'><div id="title" class="screen"><h1>Hi</h1><button data-cmd=start1 disabled>Go</button></div>
<img src="a.png"/><br></div>
</body></html>"#;

    #[test]
    fn reads_tags_attributes_and_spans() {
        let d = parse(PAGE).unwrap();
        let els = d.elements();
        let tags: Vec<&str> = els.iter().map(|e| e.tag.as_str()).collect();
        assert_eq!(
            tags,
            ["html", "head", "meta", "title", "style", "link", "body", "div", "div", "h1", "button", "img", "br"]
        );
        let style = d.find(|e| e.tag == "style").unwrap();
        assert_eq!(style.inner(PAGE), ".a > .b { color: red; }");
        let link = d.find(|e| e.tag == "link").unwrap();
        assert!(link.has_attr("data-trunk"));
        assert_eq!((link.attr("rel"), link.attr("href")), (Some("inline"), Some("hud.css")));
        let title = d.find(|e| e.attr("id") == Some("title")).unwrap();
        assert_eq!(title.inner(PAGE), "<h1>Hi</h1><button data-cmd=start1 disabled>Go</button>");
        assert_eq!(&PAGE[title.start..title.end], format!("<div id=\"title\" class=\"screen\">{}</div>", title.inner(PAGE)));
        let button = d.find(|e| e.tag == "button").unwrap();
        assert_eq!((button.attr("data-cmd"), button.attr("disabled")), (Some("start1"), Some("")));
        let ui = d.find(|e| e.attr("id") == Some("ui")).unwrap();
        assert_eq!(ui.attr("class"), Some("x"));
        assert!(ui.has_id_below());
        assert!(!title.has_id_below());
    }

    #[test]
    fn raw_text_and_comments_hide_tags() {
        let src = "<style>a::after { content: '<b>'; }</style><script>if (a<b) {}</script><!-- <p id=x> -->";
        let d = parse(src).unwrap();
        let tags: Vec<&str> = d.elements().iter().map(|e| e.tag.as_str()).collect();
        assert_eq!(tags, ["style", "script"]);
    }

    #[test]
    fn unclosed_parts_are_errors_and_missing_end_tags_close_at_the_end() {
        assert!(parse("<div class=\"a").is_err());
        assert!(parse("<style>x").is_err());
        assert!(parse("<!-- x").is_err());
        let d = parse("<div id=a><p>text").unwrap();
        let a = d.find(|e| e.attr("id") == Some("a")).unwrap();
        assert_eq!(a.end, 17);
        // A stray end tag is ignored.
        let d = parse("<div id=a></span>x</div>").unwrap();
        assert_eq!(d.find(|e| e.attr("id") == Some("a")).unwrap().inner("<div id=a></span>x</div>"), "</span>x");
    }
}
