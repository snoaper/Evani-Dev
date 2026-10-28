//! Error codes: a game's (wasm) build carries only the short code of each
//! reading error (`G14`), native builds the full text. This test keeps the
//! two in step: every `err!` in the reader has a well-formed code, each code
//! means exactly one text and each text has exactly one code, and the table
//! in docs/MODEL_FORMAT.md ("Error codes") lists every code with its text and
//! nothing else. So a code seen in a game's console always has one meaning.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

use std::collections::BTreeMap;
use std::path::Path;

/// Every `err!("CODE", "text")` in the crate's sources, with where it is.
fn uses() -> Vec<(String, String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(&src).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    for path in files {
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("err!(") {
            rest = &rest[at + 5..];
            // The macro's own definition (`err!` with `$code:literal`) is not a use.
            if rest.trim_start().starts_with('$') || !rest.trim_start().starts_with('"') {
                assert!(
                    rest.trim_start().starts_with('$'),
                    "{name}: err!( must be followed by two string literals, near {:?}",
                    &rest[..rest.len().min(40)]
                );
                continue;
            }
            let (code, after) = literal(rest, &name);
            let after = after.trim_start().strip_prefix(',').unwrap_or_else(|| panic!("{name}: err!(\"{code}\" needs a text"));
            let (msg, after) = literal(after, &name);
            assert!(after.trim_start().starts_with(')'), "{name}: err!(\"{code}\", ..) takes exactly two literals");
            out.push((code, msg, name.clone()));
            rest = after;
        }
    }
    out
}

/// Reads one plain string literal (no escapes are used in error texts).
fn literal<'a>(s: &'a str, file: &str) -> (String, &'a str) {
    let s = s.trim_start().strip_prefix('"').unwrap_or_else(|| panic!("{file}: expected a string literal"));
    let end = s.find('"').unwrap_or_else(|| panic!("{file}: unterminated string literal"));
    let lit = &s[..end];
    assert!(!lit.contains('\\'), "{file}: keep error texts free of escapes: {lit:?}");
    (lit.to_string(), &s[end + 1..])
}

#[test]
fn every_error_code_is_unique_and_has_one_text() {
    let uses = uses();
    assert!(uses.len() > 50, "found only {} err! uses; the scan is broken", uses.len());
    let mut by_code: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    let mut by_text: BTreeMap<&str, &str> = BTreeMap::new();
    for (code, text, file) in &uses {
        let mut chars = code.chars();
        let letter = chars.next().unwrap_or(' ');
        let digits: String = chars.collect();
        assert!(
            "ABEGM".contains(letter) && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) && !digits.starts_with('0'),
            "{file}: error code {code:?} must be a letter (A anim, B glb, E evani, G gltf, M meshopt) and a number"
        );
        let area = match letter {
            'A' => "anim: ",
            'B' => "glb: ",
            'E' => "evani: ",
            'G' => "gltf: ",
            _ => "meshopt: ",
        };
        assert!(text.starts_with(area), "{file}: {code} text {text:?} should start with {area:?}");
        if let Some((old, old_file)) = by_code.insert(code, (text, file)) {
            assert_eq!(old, text.as_str(), "code {code} means two things ({old_file} and {file})");
        }
        if let Some(old) = by_text.insert(text, code) {
            assert_eq!(old, code.as_str(), "text {text:?} has two codes");
        }
    }

    // The table in docs/MODEL_FORMAT.md: rows "| `CODE` | text |".
    let doc_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/MODEL_FORMAT.md");
    let doc = std::fs::read_to_string(&doc_path).unwrap();
    let section = doc.split("## Error codes").nth(1).expect("docs/MODEL_FORMAT.md needs an \"## Error codes\" section");
    let section = section.split("\n## ").next().unwrap_or(section);
    let mut documented: BTreeMap<String, String> = BTreeMap::new();
    for line in section.lines() {
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() == 2 && cells[0].starts_with('`') && cells[0].ends_with('`') && cells[0].len() > 2 {
            let code = cells[0].trim_matches('`').to_string();
            assert!(documented.insert(code.clone(), cells[1].to_string()).is_none(), "MODEL_FORMAT.md lists {code} twice");
        }
    }
    for (code, (text, _)) in &by_code {
        let row = documented.get(*code).unwrap_or_else(|| panic!("MODEL_FORMAT.md \"Error codes\" is missing {code} ({text})"));
        assert_eq!(row, text, "MODEL_FORMAT.md gives {code} a different text");
    }
    for code in documented.keys() {
        assert!(by_code.contains_key(code.as_str()), "MODEL_FORMAT.md lists {code}, which the reader no longer uses");
    }
}
