// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use std::path::{Path, PathBuf};

use memchr::memmem;

use crate::constants::*;

#[derive(Debug)]
pub struct Preprocessor {
    pub asm_dir_prefix: Option<PathBuf>,
}

// ── Byte-level scanner helpers (pure, composable) ───────────────────

/// Advances `pos` past any ASCII whitespace.
fn skip_whitespace(content: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < content.len() && content[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// If `content[pos]` is `"`, returns `Some((end, slice))` where `slice` is
/// the bytes between the quotes and `end` is the position after the closing
/// quote. Returns `None` if pos is out of bounds or doesn't start with `"`.
fn scan_quoted_string(content: &[u8], pos: usize) -> Option<(usize, &[u8])> {
    if pos >= content.len() || content[pos] != b'"' {
        return None;
    }
    let start = pos + 1;
    let mut i = start;
    while i < content.len() && content[i] != b'"' {
        i += 1;
    }
    if i >= content.len() {
        return None;
    }
    Some((i + 1, &content[start..i]))
}

/// Returns true when `b` is a valid C identifier character used in
/// INCLUDE_ASM/INCLUDE_RODATA function names: `[a-zA-Z0-9$_]`.
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'$' || b == b'_'
}

/// Scans a non-empty run of identifier bytes starting at `pos`.
/// Returns `Some((end, slice))` or `None` if no identifier bytes are found.
fn scan_identifier(content: &[u8], pos: usize) -> Option<(usize, &[u8])> {
    let mut i = pos;
    while i < content.len() && is_ident_byte(content[i]) {
        i += 1;
    }
    if i == pos {
        return None;
    }
    Some((i, &content[pos..i]))
}

/// Tries to parse an `INCLUDE_ASM(…)` or `INCLUDE_RODATA(…)` call starting
/// at `macro_start`, which should point to the `I` of `INCLUDE_`.
///
/// On success returns `Some((end, asm_dir, func_name))` where `end` is the
/// position after the closing `)`.
fn try_parse_macro(content: &[u8], macro_start: usize) -> Option<(usize, &[u8], &[u8])> {
    let mut pos = macro_start + b"INCLUDE_".len();

    // Match ASM or RODATA
    if content.get(pos..pos + 3) == Some(b"ASM") {
        pos += 3;
    } else if content.get(pos..pos + 6) == Some(b"RODATA") {
        pos += 6;
    } else {
        return None;
    }

    // Expect '('
    if content.get(pos) != Some(&b'(') {
        return None;
    }
    pos += 1;

    pos = skip_whitespace(content, pos);
    let (new_pos, asm_dir) = scan_quoted_string(content, pos)?;
    pos = skip_whitespace(content, new_pos);

    // Expect ','
    if content.get(pos) != Some(&b',') {
        return None;
    }
    pos += 1;

    pos = skip_whitespace(content, pos);
    let (new_pos, func_name) = scan_identifier(content, pos)?;
    pos = skip_whitespace(content, new_pos);

    // Expect ')'
    if content.get(pos) != Some(&b')') {
        return None;
    }
    pos += 1;

    Some((pos, asm_dir, func_name))
}

// ── Preprocessor ────────────────────────────────────────────────────

impl Preprocessor {
    pub fn new(asm_dir_prefix: Option<PathBuf>) -> Self {
        Self { asm_dir_prefix }
    }

    /// Scans `content` for `INCLUDE_ASM`/`INCLUDE_RODATA` macros and returns:
    ///
    /// - `segments`: the content split around each macro call. `segments[i]` is
    ///   the bytes before `asm_refs[i]`; `segments[n]` is the bytes after the last
    ///   macro. There are always exactly `asm_refs.len() + 1` segments.
    /// - `asm_refs`: `(asm_path, func_name)` for each macro, in source order.
    ///   The .s file is not read; only the path is resolved.
    pub fn find_macro_refs<'a>(
        &self,
        content: &'a [u8],
    ) -> (Vec<&'a [u8]>, Vec<(PathBuf, &'a str)>) {
        let finder = memmem::Finder::new(b"INCLUDE_");

        let mut segments = vec![];
        let mut asm_refs = vec![];
        let mut last_match = 0;

        // Search for INCLUDE_ anchors and try to parse a macro at each hit.
        let mut search_from = 0;
        while let Some(offset) = finder.find(&content[search_from..]) {
            let macro_start = search_from + offset;

            if let Some((end, asm_dir, func_name)) = try_parse_macro(content, macro_start) {
                segments.push(&content[last_match..macro_start]);
                last_match = end;

                // The scanner guarantees ASCII-only captures, so from_utf8 is safe.
                let asm_dir_str = std::str::from_utf8(asm_dir).unwrap();
                let func_name_str = std::str::from_utf8(func_name).unwrap();

                let asm_path = self.resolve_asm_path(asm_dir_str, func_name_str);
                asm_refs.push((asm_path, func_name_str));

                search_from = end;
            } else {
                // Not a valid macro call, skip past this INCLUDE_ and keep looking.
                search_from = macro_start + b"INCLUDE_".len();
            }
        }

        segments.push(&content[last_match..]);
        (segments, asm_refs)
    }

    fn resolve_asm_path(&self, asm_dir_str: &str, func_name_str: &str) -> PathBuf {
        let asm_dir = Path::new(asm_dir_str);
        let func_name = func_name_str.trim();

        let mut asm_path = asm_dir.join(format!("{}.s", func_name));
        if let Some(prefix) = &self.asm_dir_prefix {
            asm_path = prefix.join(&asm_path);
        }

        asm_path
    }

    /// Generates the stub C source for one assembled file given data extracted
    /// from the assembled ELF. Returns an empty string if there is nothing to emit.
    ///
    /// - `func_name`: the bare function name (without `FUNCTION_PREFIX`).
    /// - `text_byte_count`: size in bytes of the assembled `.text` section;
    ///   0 for `INCLUDE_RODATA` files that have no text.
    /// - `rodata_syms`: `(name, size_in_bytes, is_local)` for each rodata symbol,
    ///   in source order. Names are taken directly from the ELF symtab.
    pub fn stub_for(
        func_name: &str,
        text_byte_count: usize,
        rodata_syms: &[(String, usize, bool)],
    ) -> Vec<u8> {
        let mut out = Vec::new();

        out.extend_from_slice(&Self::generate_text_stub(func_name, text_byte_count));
        out.extend_from_slice(&Self::generate_rodata_stub(rodata_syms));

        out
    }

    fn generate_text_stub(func_name: &str, text_byte_count: usize) -> Vec<u8> {
        let mut out = Vec::new();
        if text_byte_count > 0 {
            let nops = text_byte_count / 4;
            out.extend_from_slice(
                format!("asm void {FUNCTION_PREFIX}{func_name}() {{\n").as_bytes(),
            );
            for _ in 0..nops {
                out.extend_from_slice(b"  nop\n");
            }
            out.extend_from_slice(b"}\n");
        }
        out
    }

    fn generate_rodata_stub(rodata_syms: &[(String, usize, bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, size, is_local) in rodata_syms {
            if *is_local {
                out.extend_from_slice(b"static ");
            };
            out.extend_from_slice(b"const unsigned char ");
            // Escape . and $ in symbol names to produce a valid C identifier.
            let c_name = name.replace('.', SYMBOL_AT).replace('$', SYMBOL_DOLLAR);
            out.extend_from_slice(c_name.as_bytes());
            out.extend_from_slice(format!("[{size}] = {{0}};\n").as_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pp() -> Preprocessor {
        Preprocessor {
            asm_dir_prefix: None,
        }
    }

    #[test]
    fn test_preprocessor_new() {
        let none_pp = Preprocessor::new(None);
        assert_eq!(none_pp.asm_dir_prefix, None);

        let some_pp = Preprocessor::new(Some(PathBuf::from("/test")));
        assert_eq!(some_pp.asm_dir_prefix, Some(PathBuf::from("/test")));
    }

    #[test]
    fn test_find_macro_refs_none() {
        let content = std::fs::read_to_string("tests/data/compiler.c").unwrap();
        let (segments, asm_refs) = pp().find_macro_refs(content.as_bytes());
        assert!(asm_refs.is_empty());
        assert_eq!(1, segments.len());
        assert_eq!(content.as_bytes(), segments[0]);
    }

    #[test]
    fn test_find_macro_refs_one() {
        let content = std::fs::read_to_string("tests/data/assembler.c").unwrap();
        let (segments, asm_refs) = pp().find_macro_refs(content.as_bytes());
        assert_eq!(1, asm_refs.len());
        assert_eq!(2, segments.len());
        assert_eq!((PathBuf::from("tests/data/Add.s"), "Add"), asm_refs[0]);
    }

    #[test]
    fn test_find_macro_refs_whitespace() {
        // Both whitespace variants must resolve to the same path and func_name.
        let inline = "#include \"common.h\"\nINCLUDE_ASM(\"tests/data\", Add);\n";
        let multiline =
            "#include \"common.h\"\nINCLUDE_ASM(\n    \"tests/data\"   ,\n    Add\n    );\n";
        let (segs_a, refs_a) = pp().find_macro_refs(inline.as_bytes());
        let (segs_b, refs_b) = pp().find_macro_refs(multiline.as_bytes());
        assert_eq!(refs_a, refs_b);
        // segments differ only in the whitespace around the macro call, not in
        // the func_name or path resolution.
        assert_eq!(segs_a.len(), segs_b.len());
    }

    // ── Scanner unit tests ──────────────────────────────────────────

    #[test]
    fn test_skip_whitespace() {
        assert_eq!(skip_whitespace(b"   hello", 0), 3);
        assert_eq!(skip_whitespace(b"hello", 0), 0);
        assert_eq!(skip_whitespace(b" \t\n\r x", 0), 5);
        assert_eq!(skip_whitespace(b"", 0), 0);
    }

    #[test]
    fn test_scan_quoted_string() {
        assert_eq!(
            scan_quoted_string(b"\"hello\" rest", 0),
            Some((7, b"hello".as_slice()))
        );
        assert_eq!(scan_quoted_string(b"no quote", 0), None);
        assert_eq!(scan_quoted_string(b"\"unterminated", 0), None);
        assert_eq!(
            scan_quoted_string(b"\"\" empty", 0),
            Some((2, b"".as_slice()))
        );
    }

    #[test]
    fn test_is_ident_byte() {
        assert!(is_ident_byte(b'a'));
        assert!(is_ident_byte(b'Z'));
        assert!(is_ident_byte(b'0'));
        assert!(is_ident_byte(b'$'));
        assert!(is_ident_byte(b'_'));
        assert!(!is_ident_byte(b' '));
        assert!(!is_ident_byte(b'('));
    }

    #[test]
    fn test_scan_identifier() {
        assert_eq!(
            scan_identifier(b"Add rest", 0),
            Some((3, b"Add".as_slice()))
        );
        assert_eq!(
            scan_identifier(b"foo$bar_9", 0),
            Some((9, b"foo$bar_9".as_slice()))
        );
        assert_eq!(scan_identifier(b"(nope", 0), None);
        assert_eq!(scan_identifier(b"", 0), None);
    }

    #[test]
    fn test_try_parse_macro_asm() {
        let input = b"INCLUDE_ASM(\"some/dir\", MyFunc)";
        let result = try_parse_macro(input, 0);
        assert!(result.is_some());
        let (end, dir, name) = result.unwrap();
        assert_eq!(end, input.len());
        assert_eq!(dir, b"some/dir");
        assert_eq!(name, b"MyFunc");
    }

    #[test]
    fn test_try_parse_macro_rodata() {
        let input = b"INCLUDE_RODATA(\"data/rodata\", my_const)";
        let result = try_parse_macro(input, 0);
        assert!(result.is_some());
        let (end, dir, name) = result.unwrap();
        assert_eq!(end, input.len());
        assert_eq!(dir, b"data/rodata");
        assert_eq!(name, b"my_const");
    }

    #[test]
    fn test_try_parse_macro_with_whitespace() {
        let input = b"INCLUDE_ASM(  \"dir\"  ,  Name  )";
        let result = try_parse_macro(input, 0);
        assert!(result.is_some());
        let (_, dir, name) = result.unwrap();
        assert_eq!(dir, b"dir");
        assert_eq!(name, b"Name");
    }

    #[test]
    fn test_try_parse_macro_invalid() {
        assert!(try_parse_macro(b"INCLUDE_OTHER(\"x\", y)", 0).is_none());
        assert!(try_parse_macro(b"INCLUDE_ASM \"x\", y)", 0).is_none());
        assert!(try_parse_macro(b"INCLUDE_ASM(x, y)", 0).is_none());
        assert!(try_parse_macro(b"INCLUDE_ASM(\"x\" y)", 0).is_none());
    }

    #[test]
    fn test_scanner_skips_non_macro_include() {
        // INCLUDE_GUARD should NOT match
        let input = b"INCLUDE_GUARD\nINCLUDE_ASM(\"dir\", Func)\n";
        let (segments, refs) = pp().find_macro_refs(input);
        assert_eq!(refs.len(), 1);
        assert_eq!(segments.len(), 2);
        assert_eq!(refs[0].1, "Func");
        // The INCLUDE_GUARD text should be retained in the first segment
        assert!(segments[0].starts_with(b"INCLUDE_GUARD"));
    }

    // ── Existing stub tests ─────────────────────────────────────────

    #[test]
    fn test_stub_for_text_only() {
        let stub_bytes = Preprocessor::stub_for("MyFunc", 12, &[]);
        let stub = String::from_utf8(stub_bytes).unwrap();
        assert!(stub.contains(&format!("asm void {FUNCTION_PREFIX}MyFunc()")));
        assert_eq!(3, stub.lines().filter(|l| l.trim() == "nop").count());
        assert!(!stub.contains("unsigned char"));
    }

    #[test]
    fn test_stub_for_rodata_only() {
        let syms = vec![("my_const".to_string(), 16, false)];
        let stub_bytes = Preprocessor::stub_for("my_const", 0, &syms);
        let stub = String::from_utf8(stub_bytes).unwrap();
        assert!(!stub.contains("asm void"));
        assert!(stub.contains("const unsigned char my_const[16]"));
        assert!(!stub.contains("static"));
    }

    #[test]
    fn test_stub_for_text_and_rodata() {
        let syms = vec![
            ("greeting".to_string(), 6, false),
            ("local_data".to_string(), 4, true),
        ];
        let stub_bytes = Preprocessor::stub_for("AsmWithRodata", 24, &syms);
        let stub = String::from_utf8(stub_bytes).unwrap();
        assert!(stub.contains(&format!("asm void {FUNCTION_PREFIX}AsmWithRodata()")));
        assert_eq!(6, stub.lines().filter(|l| l.trim() == "nop").count());
        assert!(stub.contains("const unsigned char greeting[6]"));
        assert!(stub.contains("static const unsigned char local_data[4]"));
    }

    #[test]
    fn test_stub_for_dollar_escape() {
        let syms = vec![("foo$bar".to_string(), 4, true)];
        let stub_bytes = Preprocessor::stub_for("Fn", 4, &syms);
        let stub = String::from_utf8(stub_bytes).unwrap();
        assert!(stub.contains("foo__dollar__bar"), "got: {stub}");
        assert!(!stub.contains("foo$bar"), "got: {stub}");
    }
}
