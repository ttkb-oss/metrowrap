// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE

use metrowrap::{NamedSource, SourceType, assembler, compiler, preprocessor, workspace};
use std::path::PathBuf;

fn workspace() -> workspace::Workspace {
    workspace::Workspace::new(workspace::TempMode::Normal).expect("workspace")
}

fn make_preprocessor() -> preprocessor::Preprocessor {
    preprocessor::Preprocessor::new(Some(PathBuf::from(".")))
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

#[test]
fn test_dep_asm_include() {
    let ws = workspace();
    let output = ws.path().join("asm_test.o");
    let d_file_gcc = output.with_extension("o.d");
    let d_file_mw = PathBuf::from("tests/data/asm_test.d");

    // We use -gccdep to make it easier to locate the .d file
    // In gcc mode, it goes to <output>.o.d
    let c_content = NamedSource {
        source: SourceType::Path("tests/data/asm_test.c".to_string()),
        content: b"INCLUDE_ASM(\"tests/data\", Add);\nINCLUDE_RODATA(\"tests/data\", OnlyRodata);"
            .to_vec(),
        src_dir: PathBuf::from("tests/data"),
    };

    // Ensure cleanup
    let _ = std::fs::remove_file(&d_file_gcc);
    let _ = std::fs::remove_file(&d_file_mw);

    metrowrap::process_c_file(
        &c_content,
        &output,
        &make_preprocessor(),
        &make_compiler(&["-gccdep", "-MD"]),
        &make_assembler(),
        &ws,
        false,
    )
    .expect("process_c_file");

    assert!(
        d_file_gcc.exists(),
        "Dependency file missing at {:?}",
        d_file_gcc
    );

    let content = std::fs::read_to_string(&d_file_gcc).unwrap();
    println!("Dependency content:\n{}", content);

    // Check if Add.s is in the dependency list
    assert!(
        content.contains("tests/data/Add.s"),
        "Dependency file should contain tests/data/Add.s. Content: {}",
        content
    );
    assert!(
        content.contains("tests/data/OnlyRodata.s"),
        "Dependency file should contain tests/data/OnlyRodata.s. Content: {}",
        content
    );

    let _ = std::fs::remove_file(&d_file_gcc);
}
