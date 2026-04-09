// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use std::path::PathBuf;

use metrowrap::preprocessor;

#[test]
fn test_no_include_asm() {
    let preprocessor = preprocessor::Preprocessor {
        asm_dir_prefix: None,
    };

    let content = std::fs::read("tests/data/compiler.c").unwrap();
    let (segments, asm_refs) = preprocessor.find_macro_refs(&content);

    assert!(asm_refs.is_empty(), "Expected no INCLUDE_ASM: {asm_refs:?}");
    // No macros: single segment equal to the full content.
    assert_eq!(1, segments.len());
    assert_eq!(content, segments[0]);
}

#[test]
fn test_include_one_asm() {
    let preprocessor = preprocessor::Preprocessor {
        asm_dir_prefix: None,
    };

    let content = std::fs::read("tests/data/assembler.c").unwrap();
    let (segments, asm_refs) = preprocessor.find_macro_refs(&content);

    assert_eq!(1, asm_refs.len());
    assert_eq!(2, segments.len());
    assert_eq!(
        (PathBuf::from("tests/data/Add.s"), "Add".to_string()),
        asm_refs[0]
    );

    // Whitespace variants must resolve to the same path and func_name.
    let content_ws =
        "#include \"common.h\"\n\nINCLUDE_ASM(\n    \"tests/data\"   ,\n    Add\n    );\n";
    let (_, asm_refs_ws) = preprocessor.find_macro_refs(content_ws.as_bytes());
    assert_eq!(asm_refs, asm_refs_ws);
}
