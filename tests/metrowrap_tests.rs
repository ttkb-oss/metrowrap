use std::path::PathBuf;
use std::sync::Arc;

use metrowrap;
use metrowrap::NamedSource;
use metrowrap::SourceType;
use metrowrap::assembler;
use metrowrap::compiler;
use metrowrap::preprocessor;
use metrowrap::workspace::{TempMode, Workspace};

use object::{self, Object, ObjectSection, SectionKind};

fn workspace() -> Workspace {
    Workspace::new(TempMode::Normal).expect("workspace")
}

#[test]
fn test_process_c_file() {
    let preprocessor = Arc::new(preprocessor::Preprocessor {
        asm_dir_prefix: Some(PathBuf::from(".")),
    });

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

    let assembler = assembler::Assembler {
        as_path: "mipsel-linux-gnu-as".into(),
        as_march: "allegrex".into(),
        as_mabi: "32".into(),
        as_flags: vec!["-G0".into()],
        macro_inc_path: Some("tests/data/macro.inc".into()),
    };

    let c_path = PathBuf::from("tests/data/assembler.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let result = metrowrap::process_c_file(
        &c_content,
        &PathBuf::from("target/.private/tests/metrowrap/process_c_file/assembler.o"),
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
    );

    assert!(matches!(result, Ok(())), "this is not ok: {result:?}");

    let _obj_bytes = std::fs::read("target/.private/tests/metrowrap/process_c_file/assembler.o");
}

#[test]
fn test_process_c_file_no_include_asm() {
    let preprocessor = Arc::new(preprocessor::Preprocessor {
        asm_dir_prefix: Some(PathBuf::from(".")),
    });

    let c_flags: Vec<String> = vec!["-Itests/data".to_string(), "-c".to_string()];
    let compiler = compiler::Compiler::new(
        c_flags,
        "target/.private/bin/mwccpsp.exe".into(),
        true,
        "target/.private/bin/wibo".into(),
    );

    let assembler = assembler::Assembler {
        as_path: "mipsel-linux-gnu-as".into(),
        as_march: "allegrex".into(),
        as_mabi: "32".into(),
        as_flags: vec!["-G0".into()],
        macro_inc_path: Some("tests/data/macro.inc".into()),
    };

    let c_path = PathBuf::from("tests/data/compiler.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let _result = metrowrap::process_c_file(
        &c_content,
        &PathBuf::from("target/.private/tests/metrowrap/process_c_file/compiler.o"),
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
    )
    .expect("process_c_file");

    let obj_bytes = std::fs::read("target/.private/tests/metrowrap/process_c_file/compiler.o")
        .expect("compiler.o");
    let obj = object::File::parse(&*obj_bytes).expect("no object");
    let mut sections = obj.sections();

    //  [Nr] Name              Type            Addr     Off    Size   ES Flg Lk Inf Al
    //  [ 0]                   NULL            00000000 000000 000000 00      0   0  0
    let Some(null_section) = sections.next() else {
        panic!("no NULL")
    };
    assert!(
        matches!(null_section.kind(), SectionKind::Metadata),
        "NULL Section Kind: {null_section:?}"
    );
    assert_eq!(0, null_section.size());
    assert_eq!(0, null_section.file_range().unwrap().0);

    //  [ 1] .symtab           SYMTAB          00000000 000040 000030 10      2   2  0
    let Some(symtab_section) = sections.next() else {
        panic!("no symtab")
    };
    assert!(
        matches!(symtab_section.kind(), SectionKind::Metadata),
        "SYMTAB Section Kind: {symtab_section:?}"
    );
    assert_eq!(48, symtab_section.size());
    assert_eq!(0x40, symtab_section.file_range().unwrap().0);
    assert_eq!(
        vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 3,
            0, 6, 0, 1, 0, 0, 0, 0, 0, 0, 0, 36, 0, 0, 0, 18, 0, 5, 0
        ],
        symtab_section.data().unwrap().to_vec()
    );

    //  [ 2] .strtab           STRTAB          00000000 000070 000011 00      0   0  0
    let Some(strtab_section) = sections.next() else {
        panic!("no strtab")
    };
    assert!(
        matches!(strtab_section.kind(), SectionKind::Metadata),
        "STRTAB Section Kind: {strtab_section:?}"
    );
    assert_eq!(17, strtab_section.size());
    assert_eq!(0x70, strtab_section.file_range().unwrap().0);
    assert_eq!(
        "\0Add\0.mwcats_Add\0",
        String::from_utf8(strtab_section.data().unwrap().to_vec()).unwrap()
    );

    //  [ 3] .shstrtab         STRTAB          00000000 000090 00003e 00      0   0  0
    let Some(shstrtab_section) = sections.next() else {
        panic!("no shstrtab")
    };
    assert!(
        matches!(shstrtab_section.kind(), SectionKind::Metadata),
        "SHSTRTAB Section Kind: {shstrtab_section:?}"
    );
    assert_eq!(62, shstrtab_section.size());
    assert_eq!(0x90, shstrtab_section.file_range().unwrap().0);
    assert_eq!(
        "\0.symtab\0.strtab\0.shstrtab\0.comment\0.text\0.mwcats\0.rel.mwcats\0",
        String::from_utf8(shstrtab_section.data().unwrap().to_vec()).unwrap()
    );

    //  [ 4] .comment          PROGBITS        00000000 0000e0 00001b 00      0   0  0
    //  [ 5] .text             PROGBITS        00000000 0000f0 000024 00  AX  0   0  4
    //  [ 6] .mwcats           LOUSER+0x4a2a82 00000000 000120 000008 00      5   0  4
    //  [ 7] .rel.mwcats       REL             00000000 000130 000008 08      1   6  0
}

#[test]
fn test_process_c_file_conditional_include_asm_no_include() {
    let preprocessor = Arc::new(preprocessor::Preprocessor {
        asm_dir_prefix: Some(PathBuf::from(".")),
    });

    let c_flags: Vec<String> = vec!["-Itests/data".to_string(), "-c".to_string()];
    let compiler = compiler::Compiler::new(
        c_flags,
        "target/.private/bin/mwccpsp.exe".into(),
        true,
        "target/.private/bin/wibo".into(),
    );

    let assembler = assembler::Assembler {
        as_path: "mipsel-linux-gnu-as".into(),
        as_march: "allegrex".into(),
        as_mabi: "32".into(),
        as_flags: vec!["-G0".into()],
        macro_inc_path: Some("tests/data/macro.inc".into()),
    };

    let c_path = PathBuf::from("tests/data/conditional.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output = PathBuf::from("target/.private/tests/metrowrap/process_c_file/conditional-no.o");

    let _result = metrowrap::process_c_file(
        &c_content,
        &output,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
    )
    .expect("process_c_file");

    let obj_bytes = std::fs::read(output).expect("conditional-no.o");
    let obj = object::File::parse(&*obj_bytes).expect("no object");
    let mut sections = obj.sections();

    //  [Nr] Name              Type            Addr     Off    Size   ES Flg Lk Inf Al
    //  [ 0]                   NULL            00000000 000000 000000 00      0   0  0
    let Some(null_section) = sections.next() else {
        panic!("no NULL")
    };
    assert!(
        matches!(null_section.kind(), SectionKind::Metadata),
        "NULL Section Kind: {null_section:?}"
    );
    assert_eq!(0, null_section.size());
    assert_eq!(64, null_section.file_range().unwrap().0);

    //  [ 1] .symtab           SYMTAB          00000000 000040 000030 10      2   2  0
    let Some(symtab_section) = sections.next() else {
        panic!("no symtab")
    };
    assert!(
        matches!(symtab_section.kind(), SectionKind::Metadata),
        "SYMTAB Section Kind: {symtab_section:?}"
    );
    assert_eq!(80, symtab_section.size());
    assert_eq!(0x40, symtab_section.file_range().unwrap().0);
    assert_eq!(
        vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 3,
            0, 6, 0, 22, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 3, 0, 10, 0, 1, 0, 0, 0, 0, 0, 0, 0, 36,
            0, 0, 0, 18, 0, 5, 0, 17, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 18, 0, 8, 0
        ],
        symtab_section.data().unwrap().to_vec()
    );

    //  [ 2] .strtab           STRTAB          00000000 000070 000011 00      0   0  0
    let Some(strtab_section) = sections.next() else {
        panic!("no strtab")
    };
    assert!(
        matches!(strtab_section.kind(), SectionKind::Metadata),
        "STRTAB Section Kind: {strtab_section:?}"
    );
    assert_eq!(35, strtab_section.size());
    assert_eq!(0x90, strtab_section.file_range().unwrap().0);
    assert_eq!(
        "\0Add\0.mwcats_Add\0Init\0.mwcats_Init\0",
        String::from_utf8(strtab_section.data().unwrap().to_vec()).unwrap()
    );

    //  [ 3] .shstrtab         STRTAB          00000000 000090 00003e 00      0   0  0
    let Some(shstrtab_section) = sections.next() else {
        panic!("no shstrtab")
    };
    assert!(
        matches!(shstrtab_section.kind(), SectionKind::Metadata),
        "SHSTRTAB Section Kind: {shstrtab_section:?}"
    );
    assert_eq!(98, shstrtab_section.size());
    assert_eq!(0xB3, shstrtab_section.file_range().unwrap().0);
    assert_eq!(
        "\0.symtab\0.strtab\0.shstrtab\0.comment\0.text\0.mwcats\0.rel.mwcats\0.text\0.rel.text\0.mwcats\0.rel.mwcats\0",
        String::from_utf8(shstrtab_section.data().unwrap().to_vec()).unwrap()
    );

    //  [ 4] .comment          PROGBITS        00000000 0000e0 00001b 00      0   0  0
    sections.next();
    //  [ 5] .text             PROGBITS        00000000 0000f0 000024 00  AX  0   0  4
    let Some(add_text_section) = sections.next() else {
        panic!("no text section");
    };
    assert!(matches!(add_text_section.kind(), SectionKind::Text));
    assert_eq!(36, add_text_section.size());
    assert_eq!(0x130, add_text_section.file_range().unwrap().0);
    // function implementing subtract (but named Add)
    assert_eq!(
        vec![
            224, 255, 189, 39, 0, 0, 164, 175, 16, 0, 165, 175, 0, 0, 163, 143, 16, 0, 162, 143,
            35, 16, 98, 0, 32, 0, 189, 39, 8, 0, 224, 3, 0, 0, 0, 0
        ],
        add_text_section.data().unwrap().to_vec()
    );

    //  [ 6] .mwcats           LOUSER+0x4a2a82 00000000 000120 000008 00      5   0  4
    //  [ 7] .rel.mwcats       REL             00000000 000130 000008 08      1   6  0
}

#[test]
fn test_process_c_file_conditional_include_asm_yes_include() {
    let preprocessor = Arc::new(preprocessor::Preprocessor {
        asm_dir_prefix: Some(PathBuf::from(".")),
    });

    // anable the INCLUDE_ASM line
    let c_flags: Vec<String> = vec![
        "-Itests/data".to_string(),
        "-c".to_string(),
        "-DUSE_ASM".to_string(),
    ];
    let compiler = compiler::Compiler::new(
        c_flags,
        "target/.private/bin/mwccpsp.exe".into(),
        true,
        "target/.private/bin/wibo".into(),
    );

    let assembler = assembler::Assembler {
        as_path: "mipsel-linux-gnu-as".into(),
        as_march: "allegrex".into(),
        as_mabi: "32".into(),
        as_flags: vec!["-G0".into()],
        macro_inc_path: Some("tests/data/macro.inc".into()),
    };

    let c_path = PathBuf::from("tests/data/conditional.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output = PathBuf::from("target/.private/tests/metrowrap/process_c_file/conditional-yes.o");

    let _result = metrowrap::process_c_file(
        &c_content,
        &output,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
    )
    .expect("process_c_file");

    let obj_bytes = std::fs::read(output).expect("conditional-yes.o");
    let obj = object::File::parse(&*obj_bytes).expect("no object");
    let mut sections = obj.sections();

    //  [Nr] Name              Type            Addr     Off    Size   ES Flg Lk Inf Al
    //  [ 0]                   NULL            00000000 000000 000000 00      0   0  0
    let Some(null_section) = sections.next() else {
        panic!("no NULL")
    };
    assert!(
        matches!(null_section.kind(), SectionKind::Metadata),
        "NULL Section Kind: {null_section:?}"
    );
    assert_eq!(0, null_section.size());
    assert_eq!(64, null_section.file_range().unwrap().0);

    //  [ 1] .symtab           SYMTAB          00000000 000040 000030 10      2   2  0
    let Some(symtab_section) = sections.next() else {
        panic!("no symtab")
    };
    assert!(
        matches!(symtab_section.kind(), SectionKind::Metadata),
        "SYMTAB Section Kind: {symtab_section:?}"
    );
    assert_eq!(96, symtab_section.size());
    assert_eq!(0x40, symtab_section.file_range().unwrap().0);
    assert_eq!(
        vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 3,
            0, 6, 0, 42, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 3, 0, 10, 0, 9, 0, 0, 0, 0, 0, 0, 0, 16,
            0, 0, 0, 18, 0, 5, 0, 33, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 18, 0, 8, 0, 38, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 16, 0, 0, 0
        ],
        symtab_section.data().unwrap().to_vec()
    );

    //  [ 2] .strtab           STRTAB          00000000 000070 000011 00      0   0  0
    let Some(strtab_section) = sections.next() else {
        panic!("no strtab")
    };
    assert!(
        matches!(strtab_section.kind(), SectionKind::Metadata),
        "STRTAB Section Kind: {strtab_section:?}"
    );
    assert_eq!(55, strtab_section.size());
    assert_eq!(0xA0, strtab_section.file_range().unwrap().0);
    assert_eq!(
        "\0___mw___Add\0.mwcats____mw___Add\0Init\0Add\0.mwcats_Init\0",
        String::from_utf8(strtab_section.data().unwrap().to_vec()).unwrap()
    );

    //  [ 3] .shstrtab         STRTAB          00000000 000090 00003e 00      0   0  0
    let Some(shstrtab_section) = sections.next() else {
        panic!("no shstrtab")
    };
    assert!(
        matches!(shstrtab_section.kind(), SectionKind::Metadata),
        "SHSTRTAB Section Kind: {shstrtab_section:?}"
    );
    assert_eq!(98, shstrtab_section.size());
    assert_eq!(0xD7, shstrtab_section.file_range().unwrap().0);
    assert_eq!(
        "\0.symtab\0.strtab\0.shstrtab\0.comment\0.text\0.mwcats\0.rel.mwcats\0.text\0.rel.text\0.mwcats\0.rel.mwcats\0",
        String::from_utf8(shstrtab_section.data().unwrap().to_vec()).unwrap()
    );

    //  [ 4] .comment          PROGBITS        00000000 0000e0 00001b 00      0   0  0
    sections.next();
    //  [ 5] .text             PROGBITS        00000000 0000f0 000024 00  AX  0   0  4
    let Some(add_text_section) = sections.next() else {
        panic!("no text section");
    };
    assert!(matches!(add_text_section.kind(), SectionKind::Text));
    assert_eq!(16, add_text_section.size());
    assert_eq!(0x154, add_text_section.file_range().unwrap().0);
    // include_asm with the minimal implementation
    assert_eq!(
        vec![32, 16, 133, 0, 8, 0, 224, 3, 0, 0, 0, 0, 0, 0, 0, 0],
        add_text_section.data().unwrap().to_vec()
    );

    //  [ 6] .mwcats           LOUSER+0x4a2a82 00000000 000120 000008 00      5   0  4
    //  [ 7] .rel.mwcats       REL             00000000 000130 000008 08      1   6  0
}

#[test]
fn test_rewritten_c_file() {
    let preprocessor = preprocessor::Preprocessor {
        asm_dir_prefix: Some(PathBuf::from(".")),
    };

    let c_flags: Vec<String> = vec!["-Itests/data".to_string(), "-c".to_string()];
    let compiler = compiler::Compiler::new(
        c_flags,
        "target/.private/bin/mwccpsp.exe".into(),
        true,
        "target/.private/bin/wibo".into(),
    );

    let assembler = assembler::Assembler {
        as_path: "mipsel-linux-gnu-as".into(),
        as_march: "allegrex".into(),
        as_mabi: "32".into(),
        as_flags: vec!["-G0".into()],
        macro_inc_path: Some("tests/data/macro.inc".into()),
    };

    let ws = workspace();

    // 1. Scan for INCLUDE_ASM macros - no .s file parsing.
    let content = std::fs::read("tests/data/assembler.c").expect("");
    let (segments, asm_refs) = preprocessor.find_macro_refs(&content);

    // 2. Assemble each referenced .s file and derive stub info from the ELF.
    let mut stub_source = Vec::new();
    for (i, (asm_path, func_name)) in asm_refs.iter().enumerate() {
        stub_source.extend_from_slice(&segments[i]);
        let elf = metrowrap::elf::Elf::from_bytes(
            &assembler
                .assemble_file(asm_path, ws.path())
                .expect("assembled"),
        );
        let text_byte_count = elf
            .get_functions()
            .into_iter()
            .next()
            .map(|f| f.section.data.len())
            .unwrap_or(0);
        let rodata_syms: Vec<(String, usize, bool)> = {
            use metrowrap::elf::STB_LOCAL;
            let mut syms: Vec<_> = elf
                .get_symbols()
                .iter()
                .filter(|s| s.st_name != 0 && s.st_size > 0)
                .filter(|s| {
                    elf.sections
                        .get(s.st_shndx as usize)
                        .map(|sec| sec.name == ".rodata")
                        .unwrap_or(false)
                })
                .collect();
            syms.sort_by_key(|s| s.st_shndx);
            syms.iter()
                .map(|s| (s.name.clone(), s.st_size as usize, s.bind() == STB_LOCAL))
                .collect()
        };
        stub_source.extend_from_slice(&preprocessor::Preprocessor::stub_for(
            func_name,
            text_byte_count,
            &rodata_syms,
        ));
    }
    stub_source.extend_from_slice(&segments[asm_refs.len()]);

    let temp_c = tempfile::NamedTempFile::with_suffix(".c").expect("temp_string");
    std::fs::write(temp_c.path(), stub_source).expect("temp_c file");

    // Re-compile with stubs
    let (recompiled_bytes, _) = compiler
        .compile_file(temp_c.path(), "tests/data/assembler.c", ws.path())
        .expect("recompile");
    std::fs::write("/tmp/recompiled.o", &recompiled_bytes).expect("debug file");

    let Ok(obj) = object::File::parse(&*recompiled_bytes) else {
        panic!("no object")
    };
    let mut sections = obj.sections();

    //  [Nr] Name              Type            Addr     Off    Size   ES Flg Lk Inf Al
    //  [ 0]                   NULL            00000000 000000 000000 00      0   0  0
    let Some(null_section) = sections.next() else {
        panic!("no NULL")
    };
    assert!(
        matches!(null_section.kind(), SectionKind::Metadata),
        "NULL Section Kind: {null_section:?}"
    );
    assert_eq!(0, null_section.size());
    assert_eq!(0, null_section.file_range().unwrap().0);

    //  [ 1] .symtab           SYMTAB          00000000 000040 000030 10      2   2  0
    let Some(symtab_section) = sections.next() else {
        panic!("no symtab")
    };
    assert!(
        matches!(symtab_section.kind(), SectionKind::Metadata),
        "SYMTAB Section Kind: {symtab_section:?}"
    );
    assert_eq!(48, symtab_section.size());
    assert_eq!(0x40, symtab_section.file_range().unwrap().0);
    assert_eq!(
        vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 3,
            0, 6, 0, 1, 0, 0, 0, 0, 0, 0, 0, 16, 0, 0, 0, 18, 0, 5, 0
        ],
        symtab_section.data().unwrap().to_vec()
    );

    //  [ 2] .strtab           STRTAB          00000000 000070 000021 00      0   0  0
    let Some(strtab_section) = sections.next() else {
        panic!("no strtab")
    };
    assert!(
        matches!(strtab_section.kind(), SectionKind::Metadata),
        "STRTAB Section Kind: {strtab_section:?}"
    );
    assert_eq!(33, strtab_section.size());
    assert_eq!(0x70, strtab_section.file_range().unwrap().0);
    assert_eq!(
        "\0___mw___Add\0.mwcats____mw___Add\0",
        String::from_utf8(strtab_section.data().unwrap().to_vec()).unwrap()
    );

    //  [ 3] .shstrtab         STRTAB          00000000 0000a0 00003e 00      0   0  0
    let Some(shstrtab_section) = sections.next() else {
        panic!("no shstrtab")
    };
    assert!(
        matches!(shstrtab_section.kind(), SectionKind::Metadata),
        "SHSTRTAB Section Kind: {shstrtab_section:?}"
    );
    assert_eq!(62, shstrtab_section.size());
    assert_eq!(0xa0, shstrtab_section.file_range().unwrap().0);
    assert_eq!(
        "\0.symtab\0.strtab\0.shstrtab\0.comment\0.text\0.mwcats\0.rel.mwcats\0",
        String::from_utf8(shstrtab_section.data().unwrap().to_vec()).unwrap()
    );

    //  [ 4] .comment          PROGBITS        00000000 0000e0 00001b 00      0   0  0
    let Some(comment_section) = sections.next() else {
        panic!("no comment")
    };
    assert!(
        matches!(comment_section.kind(), SectionKind::Other),
        "PROGBITS Section Kind: {comment_section:?}"
    );
    assert_eq!(27, comment_section.size());
    assert_eq!(0xe0, comment_section.file_range().unwrap().0);
    assert_eq!(
        "MW MIPS C Compiler (3.0.0)\0",
        String::from_utf8(comment_section.data().unwrap().to_vec()).unwrap()
    );

    //  [ 5] .text             PROGBITS        00000000 000100 000010 00  AX  0   0  4
    let text_section = sections.next().expect("PROGBITS section");
    assert!(
        matches!(text_section.kind(), SectionKind::Text),
        "Text Section Kind: {text_section:?}"
    );
    assert_eq!(16, text_section.size());
    assert_eq!(0x100, text_section.file_range().unwrap().0);

    //  [ 6] .mwcats           LOUSER+0x4a2a82 00000000 000110 000008 00      5   0  4
    let _mwcats_section = sections.next().expect("LOUSER section");

    //  [ 7] .rel.mwcats       REL             00000000 000120 000008 08      1   6  0
    let _rel_mwcats_section = sections.next().expect("REL section");

    let no_section = sections.next();
    assert!(
        matches!(no_section, None),
        "expected none, got: {no_section:?}"
    );
}
