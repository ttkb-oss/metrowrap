use metrowrap::compiler;
use metrowrap::workspace::{TempMode, Workspace};
use std::path::Path;

use object::{self, Object, ObjectSection, SectionKind};

fn workspace() -> Workspace {
    Workspace::new(TempMode::Normal).expect("workspace")
}

#[test]
fn test_compiler() {
    let c_flags: Vec<String> = vec![
        "-Itests/data".to_string(),
        "-c".to_string(),
        "-lang".to_string(),
        "c".to_string(),
        "-sdatathreshold".to_string(),
        "0".to_string(),
        "-char".to_string(),
        "unsigned".to_string(),
        "-fl".to_string(),
        "divbyzerocheck".to_string(),
        "-opt".to_string(),
        "nointrinsics".to_string(),
    ];
    let compiler = compiler::Compiler::new(
        c_flags,
        "target/.private/bin/mwccpsp.exe".into(),
        true,
        "target/.private/bin/wibo".into(),
    );

    let obj = compiler
        .compile_file(
            Path::new("tests/data/compiler.c"),
            "tests/data/compiler.c",
            workspace().path(),
        )
        .expect("obj");

    assert!(obj.0.len() > 0);
    assert!(matches!(obj.1, None));
}

#[test]
fn test_compiler_only_asm() {
    let c_flags: Vec<String> = vec!["-Itests/data".into(), "-c".into()];
    let compiler = compiler::Compiler::new(
        c_flags,
        "target/.private/bin/mwccpsp.exe".into(),
        true,
        "target/.private/bin/wibo".into(),
    );

    let obj_bytes = compiler
        .compile_file(
            Path::new("tests/data/assembler.c"),
            "tests/data/assembler.c",
            workspace().path(),
        )
        .expect("obj");

    let Ok(obj) = object::File::parse(&*obj_bytes.0) else {
        panic!("no object")
    };
    let mut sections = obj.sections();

    // [ 0]                   NULL            00000000 000000 000000 00      0   0  0
    let Some(null_section) = sections.next() else {
        panic!("no NULL")
    };
    assert!(
        matches!(null_section.kind(), SectionKind::Metadata),
        "NULL Section Kind: {null_section:?}"
    );
    assert_eq!(0, null_section.size());
    assert_eq!(0, null_section.file_range().unwrap().0);

    // [ 1] .symtab           SYMTAB          00000000 000040 000010 10      2   1  0
    let Some(symtab_section) = sections.next() else {
        panic!("no symtab")
    };
    assert!(
        matches!(symtab_section.kind(), SectionKind::Metadata),
        "SYMTAB Section Kind: {symtab_section:?}"
    );
    assert_eq!(16, symtab_section.size());
    assert_eq!(0x40, symtab_section.file_range().unwrap().0);
    assert_eq!(vec![0; 16], symtab_section.data().unwrap().to_vec());

    // [ 2] .strtab           STRTAB          00000000 000050 000001 00      0   0  0
    let Some(strtab_section) = sections.next() else {
        panic!("no strtab")
    };
    assert!(
        matches!(strtab_section.kind(), SectionKind::Metadata),
        "STRTAB Section Kind: {strtab_section:?}"
    );
    assert_eq!(1, strtab_section.size());
    assert_eq!(0x50, strtab_section.file_range().unwrap().0);
    assert_eq!(
        "\0",
        String::from_utf8(strtab_section.data().unwrap().to_vec()).unwrap()
    );

    // [ 3] .shstrtab         STRTAB          00000000 000060 000024 00      0   0  0
    let Some(shstrtab_section) = sections.next() else {
        panic!("no shstrtab")
    };
    assert!(
        matches!(shstrtab_section.kind(), SectionKind::Metadata),
        "SHSTRTAB Section Kind: {shstrtab_section:?}"
    );
    assert_eq!(36, shstrtab_section.size());
    assert_eq!(0x60, shstrtab_section.file_range().unwrap().0);
    assert_eq!(
        "\0.symtab\0.strtab\0.shstrtab\0.comment\0",
        String::from_utf8(shstrtab_section.data().unwrap().to_vec()).unwrap()
    );

    // [ 4] .comment          PROGBITS        00000000 000090 00001b 00      0   0  0
    let Some(comment_section) = sections.next() else {
        panic!("no comment")
    };
    assert!(
        matches!(comment_section.kind(), SectionKind::Other),
        "PROGBITS Section Kind: {comment_section:?}"
    );
    assert_eq!(27, comment_section.size());
    assert_eq!(0x90, comment_section.file_range().unwrap().0);
    assert_eq!(
        "MW MIPS C Compiler (3.0.0)\0",
        String::from_utf8(comment_section.data().unwrap().to_vec()).unwrap()
    );

    let no_section = sections.next();
    assert!(
        matches!(no_section, None),
        "expected none, got: {no_section:?}"
    );
}
