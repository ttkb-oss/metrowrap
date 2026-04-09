// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
//
// Integration tests for dependency file placement.
//
// The four flag combinations and where the final .d file must land:
//
//   -MD              →  tests/data/compiler.d      (next to source)
//   -MMD             →  tests/data/compiler.d      (next to source, sys headers excluded)
//   -gccdep -MD      →  <output>.o.d
//   -gccdep -MMD     →  <output>.o.d               (sys headers excluded)

use std::path::PathBuf;
use std::sync::Arc;

use metrowrap::NamedSource;
use metrowrap::SourceType;
use metrowrap::assembler;
use metrowrap::compiler;
use metrowrap::preprocessor;
use metrowrap::workspace::{TempMode, Workspace};

fn workspace() -> Workspace {
    Workspace::new(TempMode::Normal).expect("workspace")
}

fn make_preprocessor() -> Arc<preprocessor::Preprocessor> {
    Arc::new(preprocessor::Preprocessor {
        asm_dir_prefix: Some(PathBuf::from(".")),
    })
}

fn make_assembler() -> assembler::Assembler {
    assembler::Assembler {
        as_path: "mipsel-linux-gnu-as".into(),
        as_march: "allegrex".into(),
        as_mabi: "32".into(),
        as_flags: vec!["-G0".into()],
        macro_inc_path: Some("tests/data/macro.inc".into()),
    }
}

fn make_compiler(extra_flags: &[&str]) -> compiler::Compiler {
    let mut flags = vec!["-Itests/data".to_string(), "-c".to_string()];
    flags.extend(extra_flags.iter().map(|s| s.to_string()));
    compiler::Compiler::new(
        flags,
        "target/.private/bin/mwccpsp.exe".into(),
        true,
        "target/.private/bin/wibo".into(),
    )
}

/// compiler.c has no INCLUDE_ASM, exercising the direct-compile path.
fn compiler_source() -> NamedSource {
    let c_path = PathBuf::from("tests/data/compiler.c");
    NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    }
}

fn run(output: &PathBuf, extra_flags: &[&str]) {
    metrowrap::process_c_file(
        &compiler_source(),
        output,
        &make_preprocessor(),
        &make_compiler(extra_flags),
        &make_assembler(),
        &workspace(),
    )
    .expect("process_c_file");
}

// ─── -gccdep -MD ──────────────────────────────────────────────────────────────

#[test]
fn test_dep_gccdep_md() {
    let output = PathBuf::from("target/.private/tests/metrowrap/deps/compiler_gccdep_md.o");
    let expected = output.with_extension("o.d");
    // Dep must NOT appear next to the source.
    let unexpected = PathBuf::from("tests/data/compiler.d");

    let _ = std::fs::remove_file(&expected);
    let _ = std::fs::remove_file(&unexpected);

    run(&output, &["-gccdep", "-MD"]);

    assert!(
        expected.exists(),
        "-gccdep -MD dep file missing at {expected:?}"
    );

    let content = std::fs::read_to_string(&expected).unwrap();
    assert!(
        content.starts_with(output.to_str().unwrap()),
        "-gccdep -MD dep target wrong:\n{content}"
    );
    assert!(
        !unexpected.exists(),
        "-gccdep -MD wrote dep to wrong location {unexpected:?}"
    );

    std::fs::remove_file(&expected).unwrap();
}

// ─── -gccdep -MMD ─────────────────────────────────────────────────────────────

#[test]
fn test_dep_gccdep_mmd() {
    let output = PathBuf::from("target/.private/tests/metrowrap/deps/compiler_gccdep_mmd.o");
    let expected = output.with_extension("o.d");
    let unexpected = PathBuf::from("tests/data/compiler.d");

    let _ = std::fs::remove_file(&expected);
    let _ = std::fs::remove_file(&unexpected);

    run(&output, &["-gccdep", "-MMD"]);

    assert!(
        expected.exists(),
        "-gccdep -MMD dep file missing at {expected:?}"
    );

    let content = std::fs::read_to_string(&expected).unwrap();
    assert!(
        content.starts_with(output.to_str().unwrap()),
        "-gccdep -MMD dep target wrong:\n{content}"
    );
    assert!(
        !unexpected.exists(),
        "-gccdep -MMD wrote dep to wrong location {unexpected:?}"
    );

    std::fs::remove_file(&expected).unwrap();
}
