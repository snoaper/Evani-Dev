//! The engine-owned marked block in a Markdown file.
//!
//! ```text
//! <!-- BEGIN:evani-agent-rules engine=0.1.0 sha=0123456789abcdef (managed by `cargo xtask init-ai`; edits inside are replaced) -->
//! ...body...
//! <!-- END:evani-agent-rules -->
//! ```
//!
//! `engine` is the engine version that wrote it and `sha` the hash of the
//! body as written, so `doctor` can tell a stale block (an older engine or
//! changed guidance) from one somebody edited by hand. Everything outside the
//! markers belongs to the project and is never changed.

/// Where a new block goes when the file has none yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// At the top (the project's own text follows it).
    Top,
    /// After a leading `---` front matter (Cursor `.mdc` rules), else at the top.
    AfterFrontMatter,
}

/// What the stamp on a found block says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub engine: String,
    pub sha: String,
}

/// A block found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub stamp: Stamp,
    /// The body between the marker lines (line endings as in the file).
    pub body: String,
    /// Byte range of the whole block, both marker lines and the END line's
    /// line ending included.
    pub start: usize,
    pub end: usize,
}

/// A block compared with what this engine would write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Missing,
    Current,
    /// Written by another engine version or older guidance.
    Stale { engine: String },
    /// Its body no longer matches its own stamp: edited by hand.
    Edited,
}

fn begin_tag(id: &str) -> String {
    format!("<!-- BEGIN:{id}")
}

fn end_line(id: &str) -> String {
    format!("<!-- END:{id} -->")
}

/// The hash a body is stamped with: line endings and trailing blank lines
/// don't count, so a CRLF checkout of an LF block is still current.
pub fn body_sha(body: &str) -> String {
    let norm = normalize(body);
    crate::fnv_hex(norm.as_bytes())
}

fn normalize(body: &str) -> String {
    let lines: Vec<&str> = body.lines().map(|l| l.trim_end()).collect();
    let mut s = lines.join("\n");
    while s.ends_with('\n') {
        s.pop();
    }
    s.trim_start_matches('\n').to_string()
}

/// The whole block for `body`, with `nl` line endings.
pub fn render_block(id: &str, engine: &str, body: &str, nl: &str) -> String {
    let norm = normalize(body);
    let sha = crate::fnv_hex(norm.as_bytes());
    let mut out = format!(
        "{} engine={engine} sha={sha} (managed by `cargo xtask init-ai`; edits inside are replaced) -->{nl}",
        begin_tag(id)
    );
    for line in norm.split('\n') {
        out.push_str(line);
        out.push_str(nl);
    }
    out.push_str(&end_line(id));
    out.push_str(nl);
    out
}

/// Find the block `id` in `text`. A BEGIN without its END, or two blocks,
/// is an error: rewriting then could eat the project's own text.
pub fn find(text: &str, id: &str) -> Result<Option<Found>, String> {
    let tag = begin_tag(id);
    let Some(start) = line_start_of(text, &tag, 0) else {
        return Ok(None);
    };
    if line_start_of(text, &tag, start + tag.len()).is_some() {
        return Err(format!("two `{tag}` blocks; keep one"));
    }
    let begin_end = text[start..].find('\n').map_or(text.len(), |i| start + i + 1);
    let begin_line = text[start..begin_end].trim();
    let stamp = parse_stamp(&begin_line[tag.len()..]);
    let end_tag = end_line(id);
    let Some(end_start) = line_start_of(text, &end_tag, begin_end) else {
        return Err(format!("`{tag}` has no `{end_tag}` line"));
    };
    let after = text[end_start..].find('\n').map_or(text.len(), |i| end_start + i + 1);
    let body = text[begin_end..end_start].to_string();
    Ok(Some(Found { stamp, body, start, end: after }))
}

/// Byte offset of the first line at or after `from` that starts with `tag`.
fn line_start_of(text: &str, tag: &str, from: usize) -> Option<usize> {
    let mut i = from;
    if i > 0 && !text[..i].ends_with('\n') {
        i = text[i..].find('\n').map(|n| i + n + 1)?;
    }
    while i < text.len() {
        let line_end = text[i..].find('\n').map_or(text.len(), |n| i + n);
        if text[i..line_end].trim_start().starts_with(tag) {
            return Some(i);
        }
        i = line_end + 1;
    }
    None
}

fn parse_stamp(rest: &str) -> Stamp {
    let mut s = Stamp { engine: String::new(), sha: String::new() };
    for tok in rest.split_whitespace() {
        if let Some(v) = tok.strip_prefix("engine=") {
            s.engine = v.to_string();
        } else if let Some(v) = tok.strip_prefix("sha=") {
            s.sha = v.to_string();
        }
    }
    s
}

/// Is the block `id` in `text` what this engine would write for `body`?
pub fn state(text: &str, id: &str, engine: &str, body: &str) -> Result<State, String> {
    let Some(f) = find(text, id)? else {
        return Ok(State::Missing);
    };
    if body_sha(&f.body) != f.stamp.sha {
        return Ok(State::Edited);
    }
    if f.stamp.engine != engine || f.stamp.sha != body_sha(body) {
        return Ok(State::Stale { engine: f.stamp.engine });
    }
    Ok(State::Current)
}

/// `existing` (or a new file) with the block `id` set to `body`. Text
/// outside the markers is kept byte for byte; the file's line endings are
/// kept. Running it twice gives the same text.
pub fn upsert(existing: Option<&str>, id: &str, engine: &str, body: &str, place: Place) -> Result<String, String> {
    let text = existing.unwrap_or("");
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let block = render_block(id, engine, body, nl);
    if let Some(f) = find(text, id)? {
        return Ok(format!("{}{block}{}", &text[..f.start], &text[f.end..]));
    }
    if text.trim().is_empty() {
        return Ok(block);
    }
    let at = match place {
        Place::Top => 0,
        Place::AfterFrontMatter => front_matter_end(text).unwrap_or(0),
    };
    let (head, tail) = text.split_at(at);
    let tail = tail.trim_start_matches(['\r', '\n']);
    Ok(format!("{head}{block}{nl}{tail}"))
}

/// The byte offset just after a leading `---` ... `---` front matter.
fn front_matter_end(text: &str) -> Option<usize> {
    let first = text.lines().next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let mut i = text.find('\n')? + 1;
    while i < text.len() {
        let line_end = text[i..].find('\n').map_or(text.len(), |n| i + n);
        if text[i..line_end].trim_end() == "---" {
            return Some((line_end + 1).min(text.len()));
        }
        i = line_end + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "evani-agent-rules";

    #[test]
    fn new_file_gets_just_the_block() {
        let t = upsert(None, ID, "0.1.0", "# Rules\n- one\n", Place::Top).unwrap();
        assert!(t.starts_with("<!-- BEGIN:evani-agent-rules engine=0.1.0 sha="));
        assert!(t.ends_with("<!-- END:evani-agent-rules -->\n"));
        assert!(t.contains("\n# Rules\n- one\n<!-- END"));
        assert_eq!(state(&t, ID, "0.1.0", "# Rules\n- one").unwrap(), State::Current);
    }

    #[test]
    fn keeps_the_projects_text_and_is_idempotent() {
        let mine = "# My game\n\nMy own rule: blue karts only.\n";
        let a = upsert(Some(mine), ID, "0.1.0", "engine rules", Place::Top).unwrap();
        assert!(a.ends_with(mine), "{a}");
        let b = upsert(Some(&a), ID, "0.1.0", "engine rules", Place::Top).unwrap();
        assert_eq!(a, b, "a second run changes nothing");
        // Text below and above an existing block both survive a rewrite.
        let framed = format!("Above.\n{a}Below.\n");
        let c = upsert(Some(&framed), ID, "0.2.0", "new rules", Place::Top).unwrap();
        assert!(c.starts_with("Above.\n<!-- BEGIN:evani-agent-rules engine=0.2.0 "));
        assert!(c.contains("new rules\n<!-- END:evani-agent-rules -->\n\n# My game"));
        assert!(c.ends_with("blue karts only.\nBelow.\n"));
        assert!(!c.contains("engine rules"));
    }

    #[test]
    fn version_stamp_and_states() {
        let t = upsert(None, ID, "0.1.0", "rules v1", Place::Top).unwrap();
        assert_eq!(find(&t, ID).unwrap().unwrap().stamp.engine, "0.1.0");
        assert_eq!(state(&t, ID, "0.2.0", "rules v1").unwrap(), State::Stale { engine: "0.1.0".into() });
        assert_eq!(state(&t, ID, "0.1.0", "rules v2").unwrap(), State::Stale { engine: "0.1.0".into() });
        let edited = t.replace("rules v1", "rules v1, and mine");
        assert_eq!(state(&edited, ID, "0.1.0", "rules v1").unwrap(), State::Edited);
        assert_eq!(state("nothing here", ID, "0.1.0", "x").unwrap(), State::Missing);
    }

    #[test]
    fn crlf_files_stay_crlf_and_count_as_current() {
        let mine = "Line one\r\nLine two\r\n";
        let t = upsert(Some(mine), ID, "0.1.0", "a\nb", Place::Top).unwrap();
        assert!(!t.replace("\r\n", "").contains('\n'), "only CRLF line endings: {t:?}");
        assert_eq!(state(&t, ID, "0.1.0", "a\nb").unwrap(), State::Current);
        // An LF block checked out as CRLF is still current.
        let lf = upsert(None, ID, "0.1.0", "a\nb", Place::Top).unwrap();
        assert_eq!(state(&lf.replace('\n', "\r\n"), ID, "0.1.0", "a\nb").unwrap(), State::Current);
    }

    #[test]
    fn broken_markers_are_refused() {
        let t = "<!-- BEGIN:evani-agent-rules engine=0.1.0 sha=0 -->\nrules\nMy text\n";
        assert!(upsert(Some(t), ID, "0.1.0", "x", Place::Top).is_err(), "no END: never eat the rest");
        let one = upsert(None, ID, "0.1.0", "x", Place::Top).unwrap();
        assert!(find(&format!("{one}{one}"), ID).is_err());
    }

    #[test]
    fn front_matter_stays_first() {
        let mdc = "---\ndescription: x\nalwaysApply: true\n---\n\nOld text\n";
        let t = upsert(Some(mdc), "evani-pointer", "0.1.0", "Read AGENTS.md", Place::AfterFrontMatter).unwrap();
        assert!(t.starts_with("---\ndescription: x\nalwaysApply: true\n---\n<!-- BEGIN:evani-pointer "));
        assert!(t.ends_with("<!-- END:evani-pointer -->\n\nOld text\n"));
    }

    #[test]
    fn other_ids_are_separate() {
        let a = upsert(None, "evani-pointer", "0.1.0", "pointer", Place::Top).unwrap();
        let b = upsert(Some(&a), ID, "0.1.0", "rules", Place::Top).unwrap();
        assert!(find(&b, "evani-pointer").unwrap().is_some());
        assert!(find(&b, ID).unwrap().is_some());
    }
}
