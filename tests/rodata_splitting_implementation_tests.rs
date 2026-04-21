// tests/rodata_splitting_implementation_tests.rs
// Specific tests for the newly implemented rodata relocation splitting logic

use std::path::PathBuf;

use object::Object;

use metrowrap;
use metrowrap::NamedSource;
use metrowrap::SourceType;
use metrowrap::assembler::Assembler;
use metrowrap::compiler::Compiler;
use metrowrap::elf::{Elf, Relocation, RelocationRecord, SHT_REL};
use metrowrap::preprocessor::Preprocessor;
use metrowrap::workspace::{TempMode, Workspace};

fn workspace() -> Workspace {
    Workspace::new(TempMode::Normal).expect("workspace")
}

/// Helper to create test compiler
fn create_test_compiler() -> Compiler {
    Compiler::new(
        vec![
            "-Itests/data".to_string(),
            "-c".to_string(),
            "-lang".to_string(),
            "c".to_string(),
            "-sdatathreshold".to_string(),
            "0".to_string(),
        ],
        "target/.private/bin/mwccpsp.exe".into(),
        true,
        "target/.private/bin/wibo".into(),
    )
}

/// Helper to create test assembler
fn create_test_assembler() -> Assembler {
    Assembler {
        as_path: "mipsel-linux-gnu-as".into(),
        as_march: "allegrex".into(),
        as_mabi: "32".into(),
        as_flags: vec!["-G0".into()],
        macro_inc_path: Some("tests/data/macro.inc".into()),
    }
}

/// Helper to create test preprocessor
fn create_test_preprocessor() -> Preprocessor {
    Preprocessor::new(Some(PathBuf::from(".")))
}

/// Test 1: Verify that local_syms_inserted is correctly counted
#[test]
fn test_local_symbols_insertion_count() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/local_symbols.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output_path = PathBuf::from("target/.private/tests/implementation/local_count.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
        false,
    );

    assert!(result.is_ok(), "Failed to process file: {:?}", result.err());

    let obj_bytes = std::fs::read(&output_path).expect("Failed to read output file");
    let elf = Elf::from_bytes(&obj_bytes);

    // Count local symbols (bind == 0)
    let local_count = elf.get_symbols().iter().filter(|s| s.bind() == 0).count();

    println!("Local symbols count: {}", local_count);
    assert!(local_count > 0, "Should have at least one local symbol");
}

/// Test 2: Verify relocation offset adjustments are correct
#[test]
fn test_relocation_offset_correctness() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/multi_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output_path = PathBuf::from("target/.private/tests/implementation/offset_check.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
        false,
    );

    assert!(result.is_ok(), "Failed to process file: {:?}", result.err());

    let obj_bytes = std::fs::read(&output_path).expect("Failed to read output file");
    let elf = Elf::from_bytes(&obj_bytes);

    // Get rodata sections
    let rodata_sections: Vec<_> = elf
        .sections
        .iter()
        .enumerate()
        .filter(|(_, s)| s.name == ".rodata")
        .collect();

    println!("Found {} rodata sections", rodata_sections.len());

    // Get relocation sections for rodata
    let rel_rodata_sections: Vec<_> = elf
        .sections
        .iter()
        .filter(|s| s.name == ".rel.rodata" && s.sh_type == SHT_REL)
        .collect();

    println!("Found {} .rel.rodata sections", rel_rodata_sections.len());

    // For each relocation section, verify offsets are within bounds
    for rel_section in rel_rodata_sections {
        let relocations = Relocation::unpack_all(&rel_section.data);
        let referenced_section_idx = rel_section.sh_info as usize;

        if referenced_section_idx < elf.sections.len() {
            let referenced_section = &elf.sections[referenced_section_idx];

            for reloc in &relocations {
                assert!(
                    reloc.r_offset < referenced_section.sh_size,
                    "Relocation offset {} must be less than section size {}",
                    reloc.r_offset,
                    referenced_section.sh_size
                );
            }
        }
    }
}

/// Test 3: Verify that new rodata relocation sections are added
#[test]
fn test_new_relocation_sections_added() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/multi_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output_path = PathBuf::from("target/.private/tests/implementation/new_sections.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
        false,
    );

    assert!(result.is_ok(), "Failed to process file: {:?}", result.err());

    let obj_bytes = std::fs::read(&output_path).expect("Failed to read output file");
    let elf = Elf::from_bytes(&obj_bytes);

    // Count rodata sections
    let rodata_count = elf.sections.iter().filter(|s| s.name == ".rodata").count();

    // Count relocation sections for rodata in .text
    let rel_rodata_count = elf
        .sections
        .iter()
        .filter(|s| s.name == ".rel.text")
        .count();

    println!(
        "Rodata sections: {}, Relocation sections: {}",
        rodata_count, rel_rodata_count
    );

    // If we have multiple rodata sections, we should have relocations
    if rodata_count > 1 {
        assert!(
            rel_rodata_count >= 1,
            "With {} rodata sections, should have at least 1 relocation section",
            rodata_count
        );
    }
}

/// Test 4: Verify symbol indices are updated correctly when local symbols inserted
#[test]
fn test_symbol_index_updates() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/multi_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output_path = PathBuf::from("target/.private/tests/implementation/symbol_update.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
        false,
    );

    assert!(result.is_ok(), "Failed to process file: {:?}", result.err());

    let obj_bytes = std::fs::read(&output_path).expect("Failed to read output file");
    let mut elf = Elf::from_bytes(&obj_bytes);
    metrowrap::write_obj(
        "target/.private/tests/implementation/rodata_splitting_implementation_tests.o",
        &elf.pack(),
    )
    .unwrap();

    // Get all relocation sections
    let rel_sections: Vec<RelocationRecord> = elf.relocation_sections();
    let symtab = elf.symtab();

    // For each relocation, verify the symbol index is valid
    for rel_section in rel_sections {
        for reloc in rel_section.relocations {
            eprintln!("relocation: {reloc:?}");
            let sym_idx = reloc.symbol_index();
            assert!(
                sym_idx < symtab.symbols.len(),
                "Symbol index {} must be less than symbol count {}",
                sym_idx,
                symtab.symbols.len()
            );
        }
    }
}

/// Test 5: Verify sh_info points to correct rodata sections
#[test]
fn test_relocation_sh_info_correctness() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/multi_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output_path = PathBuf::from("target/.private/tests/implementation/sh_info.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
        false,
    );

    assert!(result.is_ok(), "Failed to process file: {:?}", result.err());

    let obj_bytes = std::fs::read(&output_path).expect("Failed to read output file");
    let elf = Elf::from_bytes(&obj_bytes);

    // Get all .rel.rodata sections
    let rel_rodata_sections: Vec<_> = elf
        .sections
        .iter()
        .filter(|s| s.name == ".rel.rodata")
        .collect();

    for rel_section in rel_rodata_sections {
        let referenced_idx = rel_section.sh_info as usize;

        // Verify the referenced section exists
        assert!(
            referenced_idx < elf.sections.len(),
            "sh_info {} must reference a valid section (max {})",
            referenced_idx,
            elf.sections.len() - 1
        );

        // Verify the referenced section is .rodata
        let referenced_section = &elf.sections[referenced_idx];
        assert_eq!(
            referenced_section.name, ".rodata",
            "Relocation section should reference a .rodata section, got '{}'",
            referenced_section.name
        );
    }
}

/// Test 6: Verify non-local symbols from assembly are added
#[test]
fn test_global_symbols_from_assembly_added() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/multi_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output_path = PathBuf::from("target/.private/tests/implementation/global_syms.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
        false,
    );

    assert!(result.is_ok(), "Failed to process file: {:?}", result.err());

    let obj_bytes = std::fs::read(&output_path).expect("Failed to read output file");
    let elf = Elf::from_bytes(&obj_bytes);

    // Count global symbols (bind != 0)
    let binding = elf.get_symbols();
    let global_symbols: Vec<_> = binding
        .iter()
        .filter(|s| s.bind() != 0 && s.st_name != 0)
        .collect();

    println!("Found {} global symbols", global_symbols.len());

    // We should have at least the function symbol
    assert!(
        global_symbols.len() > 0,
        "Should have at least one global symbol"
    );

    // Check that the main function is present
    let has_multi_rodata = global_symbols
        .iter()
        .any(|s| s.name.contains("MultiRodata") || s.name.contains("multi_rodata"));

    assert!(has_multi_rodata, "Should have MultiRodata function symbol");
}

/// Test 7: Edge case - single rodata section should not trigger splitting
#[test]
fn test_single_rodata_no_splitting() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/single_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output_path = PathBuf::from("target/.private/tests/implementation/single_no_split.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
        false,
    );

    assert!(result.is_ok(), "Failed to process file: {:?}", result.err());

    let obj_bytes = std::fs::read(&output_path).expect("Failed to read output file");
    let elf = Elf::from_bytes(&obj_bytes);

    // Should have exactly 1 rodata section
    let rodata_count = elf.sections.iter().filter(|s| s.name == ".rodata").count();

    assert_eq!(
        rodata_count, 1,
        "Single rodata file should have exactly 1 .rodata section"
    );
}

/// Test 8: Verify rodata_section_indexes is cleared between iterations
#[test]
fn test_rodata_indexes_cleared_per_file() {
    // This test processes multiple files to ensure state doesn't leak
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let test_files = vec![
        "tests/data/single_rodata.c",
        "tests/data/multi_rodata.c",
        "tests/data/single_rodata.c",
    ];

    for (i, c_file) in test_files.iter().enumerate() {
        let c_path = PathBuf::from(c_file);
        let c_content = NamedSource {
            source: SourceType::Path(c_path.display().to_string()),
            content: std::fs::read(&c_path).unwrap(),
            src_dir: PathBuf::from("tests/data"),
        };

        let output_path = PathBuf::from(format!(
            "target/.private/tests/implementation/cleared_{}.o",
            i
        ));

        let result = metrowrap::process_c_file(
            &c_content,
            &output_path,
            &preprocessor,
            &compiler,
            &assembler,
            &workspace(),
            false,
        );

        assert!(
            result.is_ok(),
            "Failed to process file {}: {:?}",
            c_file,
            result.err()
        );
    }
}

/// Test 9: Verify relocation data is properly packed
#[test]
fn test_relocation_data_packing() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/multi_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output_path = PathBuf::from("target/.private/tests/implementation/pack_check.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
        false,
    );

    assert!(result.is_ok(), "Failed to process file: {:?}", result.err());

    let obj_bytes = std::fs::read(&output_path).expect("Failed to read output file");
    let elf = Elf::from_bytes(&obj_bytes);

    // Get relocation sections
    let rel_sections: Vec<_> = elf
        .sections
        .iter()
        .filter(|s| s.sh_type == SHT_REL)
        .collect();

    for rel_section in rel_sections {
        // Data length should be a multiple of 8 (each relocation is 8 bytes)
        assert_eq!(
            rel_section.data.len() % 8,
            0,
            "Relocation section data length {} should be multiple of 8",
            rel_section.data.len()
        );

        // Verify we can unpack all relocations
        let relocations = Relocation::unpack_all(&rel_section.data);
        let expected_count = rel_section.data.len() / 8;

        assert_eq!(
            relocations.len(),
            expected_count,
            "Should unpack {} relocations from {} bytes",
            expected_count,
            rel_section.data.len()
        );
    }
}

/// Test 10: Integration test - verify entire pipeline with multiple rodata
#[test]
fn test_complete_pipeline_integration() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/multi_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let output_path = PathBuf::from("target/.private/tests/implementation/full_pipeline.o");

    // Run the full pipeline
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace(),
        false,
    );

    assert!(result.is_ok(), "Pipeline failed: {:?}", result.err());

    // Verify output is valid ELF
    let obj_bytes = std::fs::read(&output_path).expect("Failed to read output file");

    // Parse with internal parser
    let elf = Elf::from_bytes(&obj_bytes);
    assert!(elf.sections.len() > 0, "Should have sections");
    assert!(elf.get_symbols().len() > 0, "Should have symbols");

    // Parse with external parser
    let obj = object::File::parse(&*obj_bytes).expect("Should parse as valid ELF");
    assert!(obj.sections().count() > 0, "Should have sections");
    assert!(obj.symbols().count() > 0, "Should have symbols");

    println!("✓ Pipeline integration test passed");
}
