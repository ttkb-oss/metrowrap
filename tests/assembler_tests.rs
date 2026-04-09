use metrowrap::assembler;
use metrowrap::workspace::{TempMode, Workspace};
use object::{self, Object, ObjectSection, SectionKind};

fn workspace() -> Workspace {
    Workspace::new(TempMode::Normal).expect("workspace")
}

#[test]
fn test_assembler() {
    let assembler = assembler::Assembler {
        as_path: "mipsel-linux-gnu-as".into(),
        as_march: "allegrex".into(),
        as_mabi: "32".into(),
        as_flags: vec!["-G0".into()],
        macro_inc_path: Some("tests/data/macro.inc".into()),
    };

    let ws = workspace();
    let asm_bytes = assembler
        .assemble_file("tests/data/Add.s", ws.path())
        .expect("asm");

    assert!(asm_bytes.len() > 0);

    let obj = object::File::parse(&*asm_bytes).expect("object");
    let mut sections = obj.sections();

    // [Nr] Name              Type            Addr     Off    Size   ES Flg Lk Inf Al
    // [ 0]                   NULL            00000000 000000 000000 00      0   0  0
    let null_section = sections.next().expect("NULL section");
    assert!(
        matches!(null_section.kind(), SectionKind::Metadata),
        "NULL Section Kind: {null_section:?}"
    );
    assert_eq!(0, null_section.size());
    assert_eq!(0, null_section.file_range().unwrap().0);

    // [ 1] .text             PROGBITS        00000000 000040 000010 00  AX  0   0 16
    let text_section = sections.next().expect("PROGBITS section");
    assert!(
        matches!(text_section.kind(), SectionKind::Text),
        "Text Section Kind: {text_section:?}"
    );
    assert_eq!(16, text_section.size());
    assert_eq!(0x40, text_section.file_range().unwrap().0);

    // [ 2] .data             PROGBITS        00000000 000050 000000 00  WA  0   0 16
    let data_section = sections.next().expect("PROGBITS section");
    assert!(
        matches!(data_section.kind(), SectionKind::Data),
        "Data Section Kind: {text_section:?}"
    );
    assert_eq!(0, data_section.size());
    assert_eq!(0x50, data_section.file_range().unwrap().0);

    // [ 3] .bss              NOBITS          00000000 000050 000000 00  WA  0   0 16
    let bss_section = sections.next().expect("NOBITS section");
    assert!(
        matches!(bss_section.kind(), SectionKind::UninitializedData),
        "BSS Section Kind: {text_section:?}"
    );
    assert_eq!(0, bss_section.size());
    assert!(matches!(bss_section.file_range(), None));

    // [ 4] .reginfo          MIPS_REGINFO    00000000 000050 000018 18   A  0   0  4
    let _reginfo_section = sections.next().expect("MIPS_REGINFO section");

    // [ 5] .MIPS.abiflags    MIPS_ABIFLAGS   00000000 000068 000018 18   A  0   0  8
    let _mips_abiflags_section = sections.next().expect("MIPS_ABIFLAGS section");

    // [ 6] .pdr              PROGBITS        00000000 000080 000000 00      0   0  4
    let _pdr_section = sections.next().expect("PROGBITS section");

    // [ 7] .gnu.attributes   GNU_ATTRIBUTES  00000000 000080 000010 00      0   0  1
    let _gnu_attributes_section = sections.next().expect("GNU_ATTRIBUTES section");

    // [ 8] .symtab           SYMTAB          00000000 000090 000090 10      9   8  4
    let _symtab_section = sections.next().expect("SYMTAB section");

    // [ 9] .strtab           STRTAB          00000000 000120 000005 00      0   0  1
    let _strtab_section = sections.next().expect("STRTAB section");

    // [10] .shstrtab         STRTAB          00000000 000125 000059 00      0   0  1
    let _shstrtab_section = sections.next().expect("STRTAB section");

    let no_section = sections.next();
    assert!(
        matches!(no_section, None),
        "expected none, got: {no_section:?}"
    );
}

#[test]
fn test_assembler_macro_inc_no_parent_dir() {
    let assembler = assembler::Assembler {
        as_path: "mipsel-linux-gnu-as".into(),
        as_march: "allegrex".into(),
        as_mabi: "32".into(),
        as_flags: vec!["-G0".into()],
        macro_inc_path: Some("macro.inc".into()), // bare filename, no dir component
    };

    let ws = workspace();
    // NoMacros.s doesn't reference any macros from macro.inc so missing -I is fine.
    let result = assembler.assemble_data(
        std::fs::File::open("tests/data/NoMacros.s").expect("NoMacros.s"),
        ws.path(),
    );
    assert!(result.is_ok(), "unexpected error: {result:?}");
}

#[test]
fn test_assembler_failure() {
    let assembler = assembler::Assembler {
        as_path: "mipsel-linux-gnu-as".into(),
        as_march: "allegrex".into(),
        as_mabi: "32".into(),
        as_flags: vec!["-G0".into()],
        macro_inc_path: None,
    };

    let ws = workspace();
    let bad_asm = b"this is not valid assembly\n".as_ref();
    let result = assembler.assemble_data(bad_asm, ws.path());
    assert!(
        matches!(result, Err(metrowrap::error::MWError::Assembler(_))),
        "expected Assembler error, got: {result:?}"
    );
}
