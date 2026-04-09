// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use memchr::memmem;
use regex::bytes::Regex;

use crate::constants::*;

#[derive(Debug)]
pub struct Preprocessor {
    pub asm_dir_prefix: Option<PathBuf>,
}

static INCLUDE_REGEX: OnceLock<Regex> = OnceLock::new();
fn include_regex() -> &'static Regex {
    INCLUDE_REGEX.get_or_init(|| {
        Regex::new(r#"INCLUDE_(?:ASM|RODATA)\(\s*"([^"]*)"\s*,\s*([a-zA-Z0-9$_]*)\s*\)"#).unwrap()
    })
}

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
    ) -> (Vec<&'a [u8]>, Vec<(PathBuf, String)>) {
        // if `INCLUDE_` doesn't exist in the string, skip everything
        if memmem::find(content, b"INCLUDE_").is_none() {
            return (vec![content], vec![]);
        }

        let mut segments = vec![];
        let mut asm_refs = vec![];
        let mut last_match = 0;

        for caps in include_regex().captures_iter(content) {
            let m = caps.get(0).unwrap();
            segments.push(&content[last_match..m.start()]);
            last_match = m.end();

            // The inner captures are guaranteed to be ASCII by the regex bounds,
            // so from_utf8 will safely unwrap.
            let asm_dir_str = std::str::from_utf8(&caps[1]).unwrap();
            let func_name_str = std::str::from_utf8(&caps[2]).unwrap();

            let asm_dir = Path::new(asm_dir_str);
            let func_name = func_name_str.trim().to_string();

            let mut asm_path = asm_dir.join(format!("{}.s", func_name));
            if let Some(prefix) = &self.asm_dir_prefix {
                asm_path = prefix.join(&asm_path);
            }

            asm_refs.push((asm_path, func_name));
        }
        segments.push(&content[last_match..]);

        (segments, asm_refs)
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

        for (name, size, is_local) in rodata_syms {
            if *is_local {
                out.extend_from_slice("static ".as_bytes());
            };
            out.extend_from_slice("const unsigned char ".as_bytes());
            // Escape . and $ in symbol names to produce a valid C identifier.
            let c_name = name.replace('.', SYMBOL_AT).replace('$', SYMBOL_DOLLAR);
            out.extend_from_slice(c_name.as_bytes());
            out.extend_from_slice(format!("[{size}]").as_bytes());
            out.extend_from_slice(" = {{0}};\n".as_bytes());
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
        assert_eq!(
            (PathBuf::from("tests/data/Add.s"), "Add".to_string()),
            asm_refs[0]
        );
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
