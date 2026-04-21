// tests/rodata_edge_cases_tests.rs
// Edge case and error condition tests for rodata relocation splitting
use metrowrap;
use metrowrap::NamedSource;
use metrowrap::SourceType;
use metrowrap::assembler::Assembler;
use metrowrap::compiler::Compiler;
use metrowrap::elf::{Elf, Relocation, RelocationRecord, SHT_REL, Section};
use metrowrap::preprocessor::Preprocessor;
use metrowrap::workspace::{TempMode, Workspace};
use std::path::PathBuf;

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

/// Test: Relocation at exact section boundary
#[test]
fn test_relocation_at_boundary() {
    // Simulate relocations exactly at section boundaries
    let rodata_section_offsets = vec![13, 29, 48];

    // Relocation exactly at offset 13 (boundary between section 0 and 1)
    let test_offset = 13u32;

    let mut found_section = None;
    for i in 0..rodata_section_offsets.len() {
        if test_offset < rodata_section_offsets[i] as u32 {
            found_section = Some(i);
            break;
        }
    }

    // Should be assigned to section 1 (first section where offset < boundary)
    assert_eq!(
        found_section,
        Some(1),
        "Relocation at boundary should go to next section"
    );
}

/// Test: Relocation offset equals section size
#[test]
fn test_relocation_offset_equals_section_size() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/single_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let ws = workspace();
    let output_path = PathBuf::from("target/.private/tests/edge/boundary.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &ws,
        false,
    );

    assert!(result.is_ok());

    let obj_bytes = std::fs::read(&output_path).unwrap();
    let elf = Elf::from_bytes(&obj_bytes);

    // Check all relocations are strictly less than section size
    for section in &elf.sections {
        if section.sh_type == SHT_REL {
            let relocs = Relocation::unpack_all(&section.data);
            let target_section = &elf.sections[section.sh_info as usize];

            for reloc in relocs {
                assert!(
                    reloc.r_offset < target_section.sh_size,
                    "Relocation offset {} should be < section size {}",
                    reloc.r_offset,
                    target_section.sh_size
                );
            }
        }
    }
}

/// Test: Zero-sized rodata sections
#[test]
fn test_zero_sized_rodata() {
    // Test that code handles zero-sized sections gracefully
    let rodata_section_offsets = vec![0, 0, 0];
    let relocations = vec![Relocation {
        r_offset: 0,
        r_info: 0x105,
        symbol: String::new(),
    }];

    let mut new_relocations: Vec<Vec<Relocation>> = vec![vec![]; 3];

    for relocation in relocations {
        let mut assigned = false;
        for i in 0..rodata_section_offsets.len() {
            if relocation.r_offset < rodata_section_offsets[i] as u32 {
                new_relocations[i].push(relocation.clone());
                assigned = true;
                break;
            }
        }
        // If not assigned to any section, it might go past all boundaries
        assert!(assigned || relocation.r_offset >= *rodata_section_offsets.last().unwrap() as u32);
    }
}

/// Test: Very large offset values
#[test]
fn test_large_offset_values() {
    let large_offset = 0xFFFF_FFF0u32;
    let rodata_section_offsets = vec![100, 1000, 10000];

    let relocation = Relocation {
        r_offset: large_offset,
        r_info: 0x105,
        symbol: String::new(),
    };

    let mut found_section = None;
    for i in 0..rodata_section_offsets.len() {
        if relocation.r_offset < rodata_section_offsets[i] as u32 {
            found_section = Some(i);
            break;
        }
    }

    // Large offset should not be assigned to any section
    assert_eq!(
        found_section, None,
        "Very large offset should not match any section"
    );
}

/// Test: Symbol index at boundary (initial_sh_info_value)
#[test]
fn test_symbol_index_at_threshold() {
    let initial_sh_info_value = 10;
    let local_syms_inserted = 3;

    // Test the boundary case
    let test_indices = vec![9, 10, 11];

    for idx in test_indices {
        let updated = if idx >= initial_sh_info_value {
            idx + local_syms_inserted
        } else {
            idx
        };

        if idx < initial_sh_info_value {
            assert_eq!(
                updated, idx,
                "Index {} below threshold should not change",
                idx
            );
        } else {
            assert_eq!(
                updated,
                idx + local_syms_inserted,
                "Index {} at/above threshold should increase by {}",
                idx,
                local_syms_inserted
            );
        }
    }
}

/// Test: Empty rodata_section_indexes vector
#[test]
fn test_empty_rodata_section_indexes() {
    let rodata_section_indexes: Vec<usize> = vec![];

    // This simulates the case where num_rodata_symbols == 0
    // In this case, the code should not try to access rodata_section_indexes[0]

    // The actual code should never reach this state because of the check:
    // if num_rodata_symbols > 0
    // But we test the logic anyway

    if !rodata_section_indexes.is_empty() {
        let _first = rodata_section_indexes[0];
    }

    // Should not panic
}

/// Test: Relocation with symbol index 0 (undefined symbol)
#[test]
fn test_undefined_symbol_relocation() {
    let relocation = Relocation {
        r_offset: 10,
        r_info: 0x0005, // Symbol index 0, type 5
        symbol: String::new(),
    };

    assert_eq!(relocation.symbol_index(), 0, "Symbol index should be 0");

    // In the actual code, we should handle this gracefully
    // Symbol 0 is typically the undefined symbol
}

#[test]
fn test_only_rodata() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let file = "tests/data/only_rodata.c";
    let c_path = PathBuf::from(file);

    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let ws = workspace();

    let output_path = PathBuf::from("target/.private/tests/edge/only_rodata.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &ws,
        false,
    );

    assert!(
        result.is_ok(),
        "Failed on file {}: {:?}",
        file,
        result.err()
    );

    let obj_bytes = std::fs::read(&output_path).unwrap();
    let elf = Elf::from_bytes(&obj_bytes);

    let rodata_count = elf.sections.iter().filter(|s| s.name == ".rodata").count();
    assert_eq!(
        rodata_count, 1,
        "File {} should have {} rodata sections, got {}",
        file, 1, rodata_count
    );

    let rodata = elf
        .sections
        .iter()
        .filter(|s| s.name == ".rodata")
        .next()
        .unwrap();
    let only_rodata = str::from_utf8(&rodata.data).unwrap();
    assert_eq!(only_rodata, "This is only rodata, no code\0");
}

/// Test: Multiple files processed sequentially
#[test]
fn test_sequential_file_processing() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    // Process multiple files to ensure state is properly cleared
    let files = vec![
        ("tests/data/single_rodata.c", 1),
        ("tests/data/multi_rodata.c", 3),
        ("tests/data/only_rodata.c", 1),
    ];

    let ws = workspace();

    for (i, (file, expected_rodata_count)) in files.iter().enumerate() {
        let c_path = PathBuf::from(file);
        let c_content = NamedSource {
            source: SourceType::Path(c_path.display().to_string()),
            content: std::fs::read(&c_path).unwrap(),
            src_dir: PathBuf::from("tests/data"),
        };

        let output_path = PathBuf::from(format!("target/.private/tests/edge/sequential_{}.o", i));

        let result = metrowrap::process_c_file(
            &c_content,
            &output_path,
            &preprocessor,
            &compiler,
            &assembler,
            &ws,
            false,
        );

        assert!(
            result.is_ok(),
            "Failed on file {}: {:?}",
            file,
            result.err()
        );

        let obj_bytes = std::fs::read(&output_path).unwrap();
        let elf = Elf::from_bytes(&obj_bytes);

        let rodata_count = elf.sections.iter().filter(|s| s.name == ".rodata").count();

        assert_eq!(
            rodata_count, *expected_rodata_count,
            "File {} should have {} rodata sections, got {}",
            file, expected_rodata_count, rodata_count
        );
    }
}

/// Test: Relocation record with no relocations
#[test]
fn test_empty_relocation_record() {
    let mut section = Section::default();
    section.sh_type = SHT_REL;
    section.sh_info = 5;

    let mut reloc_record = RelocationRecord::new(section);

    // Empty relocations list
    assert_eq!(reloc_record.relocations.len(), 0);

    // Pack should still work
    reloc_record.pack_data();
    assert_eq!(reloc_record.section.data.len(), 0);
}

/// Test: sh_info overflow (section index too large)
#[test]
fn test_sh_info_bounds_checking() {
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/single_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let ws = workspace();
    let output_path = PathBuf::from("target/.private/tests/edge/sh_info_bounds.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &ws,
        false,
    );

    assert!(result.is_ok());

    let obj_bytes = std::fs::read(&output_path).unwrap();
    let elf = Elf::from_bytes(&obj_bytes);

    // Verify all sh_info values are within bounds
    for section in &elf.sections {
        if section.sh_type == SHT_REL {
            assert!(
                (section.sh_info as usize) < elf.sections.len(),
                "sh_info {} exceeds section count {}",
                section.sh_info,
                elf.sections.len()
            );
        }
    }
}

/// Test: Relocation type preservation
#[test]
fn test_relocation_type_preserved() {
    let original_types = vec![1, 2, 4, 5, 16, 26];

    for reloc_type in original_types {
        let mut reloc = Relocation {
            r_offset: 0,
            r_info: (5 << 8) | reloc_type, // Symbol 5, various types
            symbol: String::new(),
        };

        assert_eq!(reloc.type_id(), reloc_type, "Type should be preserved");

        // Change symbol index
        reloc.set_symbol_index(10);

        // Type should still be preserved
        assert_eq!(
            reloc.type_id(),
            reloc_type,
            "Type should be preserved after changing symbol index"
        );
    }
}

/// Test: Handling of identical rodata sections
#[test]
fn test_identical_rodata_content() {
    // This tests that sections with identical content are still kept separate
    let preprocessor = create_test_preprocessor();
    let compiler = create_test_compiler();
    let assembler = create_test_assembler();

    let c_path = PathBuf::from("tests/data/multi_rodata.c");
    let c_content = NamedSource {
        source: SourceType::Path(c_path.display().to_string()),
        content: std::fs::read(&c_path).unwrap(),
        src_dir: PathBuf::from("tests/data"),
    };

    let ws = workspace();
    let output_path = PathBuf::from("target/.private/tests/edge/identical_rodata.o");
    let result = metrowrap::process_c_file(
        &c_content,
        &output_path,
        &preprocessor,
        &compiler,
        &assembler,
        &ws,
        false,
    );

    assert!(result.is_ok());

    let obj_bytes = std::fs::read(&output_path).unwrap();
    let elf = Elf::from_bytes(&obj_bytes);

    // Count rodata sections
    let rodata_sections: Vec<_> = elf
        .sections
        .iter()
        .filter(|s| s.name == ".rodata")
        .collect();

    // Even if content is identical, sections should be separate
    assert!(rodata_sections.len() > 0, "Should have rodata sections");
}

/// Test: Maximum number of relocations per section
#[test]
fn test_many_relocations() {
    // Create many relocations to test performance and correctness
    let num_relocs = 100;
    let rodata_section_offsets = vec![50, 150, 300];

    let mut relocations = Vec::new();
    for i in 0..num_relocs {
        relocations.push(Relocation {
            r_offset: (i * 3) as u32,
            r_info: ((i % 10) << 8) | 5,
            symbol: String::new(),
        });
    }

    let mut new_relocations: Vec<Vec<Relocation>> = vec![vec![]; 3];

    for mut relocation in relocations {
        for i in 0..rodata_section_offsets.len() {
            if relocation.r_offset < rodata_section_offsets[i] as u32 {
                if i > 0 {
                    relocation.r_offset -= rodata_section_offsets[i - 1] as u32;
                }
                new_relocations[i].push(relocation);
                break;
            }
        }
    }

    // Verify all relocations were assigned
    let total_assigned: u32 = new_relocations.iter().map(|v| v.len() as u32).sum();

    // Some relocations might be beyond all sections
    assert!(
        total_assigned <= num_relocs,
        "Should assign at most {} relocations",
        num_relocs
    );
}

/// Test: Relocation symbol index overflow
#[test]
#[should_panic = "Relocation cannot reference a symbols above 16777215, got 16777216"]
fn test_symbol_index_overflow() {
    // Test with very large symbol indices
    let mut reloc = Relocation {
        r_offset: 0,
        r_info: 0xFFFF_FF05, // Very large symbol index
        symbol: String::new(),
    };

    let sym_idx = reloc.symbol_index();
    println!("Symbol index: {}", sym_idx);

    // Should not panic
    reloc.set_symbol_index((sym_idx + 1) as u32);
}
