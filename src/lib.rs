// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
pub mod assembler;
pub mod compiler;
pub mod constants;
pub mod elf;
pub mod error;
pub mod makerule;
pub mod preprocessor;

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock;

use encoding_rs::Encoding;
use rayon::prelude::*;
use tempfile::Builder;

use crate::assembler::Assembler;
use crate::compiler::Compiler;
use crate::constants::*;
use crate::elf::Elf;
use crate::elf::Section;
use crate::elf::section::{Relocation, RelocationRecord};
use crate::makerule::MakeRule;
use crate::preprocessor::Preprocessor;

#[macro_export]
macro_rules! strings {
    ($($str:expr),*) => ({
        vec![$(String::from($str),)*] as Vec<String>
    });
}

pub fn escape_symbol(name: &str) -> String {
    name.replace(".", SYMBOL_AT).replace("$", SYMBOL_DOLLAR)
}

pub fn unescape_symbol(name: &str) -> String {
    name.replace(SYMBOL_AT, ".").replace(SYMBOL_DOLLAR, "$")
}

pub enum SourceType {
    StdIn,
    Path(String),
}

static STDIN_NAME: LazyLock<String> = LazyLock::new(|| String::from("<stdin>"));

impl SourceType {
    fn name(&self) -> &String {
        match self {
            Self::StdIn => &STDIN_NAME,
            Self::Path(s) => s,
        }
    }
}

pub struct NamedString {
    pub source: SourceType,
    pub content: String,
    pub encoding: &'static Encoding,
    pub src_dir: PathBuf,
}

impl NamedString {
    fn with_tmp<F, T>(&self, f: F) -> Result<T, Box<dyn Error>>
    where
        F: FnOnce(&Path) -> Result<T, Box<dyn Error>>,
    {
        // TODO this should really check and do this only
        //      for stdin. relatively expensive

        let (output, _, failure) = self.encoding.encode(&self.content);

        if failure {
            panic!(
                "Could not encode {} as {}",
                self.source.name(),
                self.encoding.name()
            );
        }

        let c_file = Builder::new().suffix(".c").tempfile_in(&self.src_dir)?;
        std::fs::write(c_file.path(), &output)?;
        let r = f(c_file.path());
        if let Err(e) = r {
            eprintln!(
                "Error occurred, temporary file available at {}",
                c_file.path().display()
            );
            // TODO: make saving intermediate temp files an option
            // std::mem::forget(c_file);
            return Err(e);
        }
        r
    }
}

pub fn process_c_file(
    c_content: &NamedString,
    o_file: &Path, // TODO: make a WRITER
    preprocessor: &Arc<Preprocessor>,
    compiler: &Compiler,
    assembler: &Assembler,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Preprocess to find INCLUDE_ASM macros and produce stub source.
    let (new_lines, asm_files) = preprocessor.find_macros(&c_content.content);

    if asm_files.is_empty() {
        // No INCLUDE_ASM macros: compile the original file directly and we're done.
        let (obj_bytes, make_rule) = c_content
            .with_tmp(|c_file| Ok(compiler.compile_file(c_file, c_content.source.name())?))?;
        write_dependency_file(compiler, make_rule, c_content, o_file)?;
        return write_obj(o_file, &obj_bytes);
    }

    // 3. Create temp C file with stubs
    let temp_c = Builder::new()
        .suffix(".c")
        .tempfile_in(&c_content.src_dir)?;
    std::fs::write(temp_c.path(), new_lines)?;

    let (recompiled_bytes, make_rule) =
        compiler.compile_file(temp_c.path(), c_content.source.name())?;
    let mut compiled_elf = Elf::from_bytes(&recompiled_bytes);

    let rel_text_sh_name = compiled_elf.add_sh_symbol(".rel.text".to_string());

    let stub_functions: HashSet<String> = compiled_elf
        .function_names()
        .iter()
        .filter_map(|name| name.strip_prefix(FUNCTION_PREFIX))
        .map(str::to_string)
        .collect();

    compiled_elf.symbol_cleanup();

    let symbol_to_section_idx: HashMap<String, u16> = compiled_elf
        .symtab()
        .symbols
        .iter()
        .map(|sym| (sym.name.clone(), sym.st_shndx))
        .collect();

    let asm_objects: Vec<(&PathBuf, usize, Vec<u8>)> = asm_files
        .par_iter()
        .map(|(asm_file, num_rodata_symbols)| {
            let assembled_bytes = assembler.assemble_file(asm_file).expect("assembled bytes");
            (asm_file, *num_rodata_symbols, assembled_bytes)
        })
        .collect();

    for (asm_file, num_rodata_symbols, assembled_bytes) in asm_objects {
        let function = asm_file.file_stem().unwrap().to_str().unwrap();

        let mut assembled_elf = Elf::from_bytes(&assembled_bytes);

        let asm_functions = assembled_elf.get_functions();
        assert_eq!(1, asm_functions.len());

        let asm_text = &asm_functions[0].section.data;

        // if this is a function and that function is not an INCLUDE_ASM, ignore
        let asm_main_symbol = asm_file.file_stem().unwrap().display().to_string();
        if !asm_text.is_empty() && !stub_functions.contains(&asm_main_symbol) {
            continue;
        }

        let mut rodata_section_indices: Vec<usize> = vec![];
        let mut text_section_index: usize = 0xFFFFFFFF;

        if !asm_text.is_empty() {
            text_section_index = compiled_elf.text_section_by_name(function);

            // assumption is that .rodata will immediately follow the .text section
            if num_rodata_symbols > 0 {
                let mut i = text_section_index + 1;
                for section in &compiled_elf.sections[i..] {
                    if section.name == ".rodata" {
                        rodata_section_indices.push(i);
                        if rodata_section_indices.len() == num_rodata_symbols {
                            break;
                        }
                    }
                    i += 1;
                }
            }

            assert_eq!(
                num_rodata_symbols,
                rodata_section_indices.len(),
                ".rodata section count mismatch"
            );

            // transplant .text section data from assembled object
            let text_section = &mut compiled_elf.sections[text_section_index];
            assert!(
                asm_text.len() >= text_section.data.len(),
                "Not enough assembly to fill {function} in {}",
                c_content.source.name()
            );
            text_section.data = asm_text[..text_section.data.len()].to_vec();
            if text_section.data.len() < text_section.sh_size as usize {
                let needed_bytes: usize = text_section.sh_size as usize - text_section.data.len();
                text_section.data.extend(vec![0u8; needed_bytes]);
            }
        } else {
            // this file only contains rodata
            assert_eq!(1, num_rodata_symbols);
            let idx = symbol_to_section_idx[function];
            rodata_section_indices.push(idx.into());
        }

        let mut rodata_section_offsets: Vec<usize> = vec![];

        let rel_rodata_sh_name = if num_rodata_symbols > 0 {
            let rodata_sections = assembled_elf.rodata_sections();
            assert_eq!(
                1,
                rodata_sections.len(),
                "Expected ASM to contain 1 .rodata section, found {}",
                rodata_sections.len()
            );

            let asm_rodata = rodata_sections[0];
            let mut offset: usize = 0;
            for idx in &rodata_section_indices {
                // copy slices of rodata from ASM object into each .rodata section
                let data_len = compiled_elf.sections[*idx].data.len();
                compiled_elf.sections[*idx].data =
                    asm_rodata.data[offset..(offset + data_len)].to_vec();
                offset += data_len;
                rodata_section_offsets.push(offset);

                // force 4-byte alignment for .rodata sections (defaults to 16-byte)
                compiled_elf.sections[*idx].sh_addralign = 2;
            }

            compiled_elf.add_sh_symbol(".rel.rodata")
        } else {
            0xFFFFFFFFu32
        };

        let relocation_records = assembled_elf.reloc_sections();
        assert!(
            relocation_records.len() < 3,
            "{} has too many relocation records",
            asm_file.display()
        );
        let mut reloc_symbols: HashSet<String> = HashSet::new();

        let initial_sh_info_value = compiled_elf.symtab().section.sh_info;
        let mut local_syms_inserted: usize = 0;

        // assumes .text relocations precede .rodata relocations
        for (i, (_, relocation_record)) in relocation_records.into_iter().enumerate() {
            let mut relocation_record = relocation_record.clone();
            relocation_record.sh_link = compiled_elf.symtab_idx as u32;
            if !asm_text.is_empty() && i == 0 {
                relocation_record.sh_name = rel_text_sh_name;
                relocation_record.sh_info = text_section_index as u32;
            } else {
                relocation_record.sh_name = rel_rodata_sh_name;
                relocation_record.sh_info = rodata_section_indices[0] as u32;
            }

            let mut assembled_symtab = assembled_elf.symtab().clone();
            let mut rr = RelocationRecord::new(relocation_record);

            for relocation in &mut rr.relocations {
                let symbol = &mut assembled_symtab.symbols[relocation.symbol_index()];
                if symbol.bind() == 0 {
                    local_syms_inserted += 1;
                }

                let force = asm_text.is_empty() || i != 0;
                if !asm_text.is_empty() && i == 1 {
                    // repoint .rodata reloc to .text section
                    symbol.st_shndx = text_section_index as u16;
                }

                let index = compiled_elf.add_symbol_get_index(symbol.clone(), force) as u32;
                relocation.set_symbol_index(index);
                reloc_symbols.insert(symbol.name.clone());
            }
            rr.pack();
            assembled_elf.set_symtab(&assembled_symtab);
            compiled_elf.add_section(rr.section);
        }

        let mut new_rodata_relocs: Vec<Section> = vec![];
        if local_syms_inserted > 0 {
            // update relocations
            let relocation_sections = compiled_elf.reloc_sections();
            for (idx, relocation_section) in relocation_sections {
                let mut relocation_record = RelocationRecord::new(relocation_section.clone());

                // Check if this is a rodata relocation that needs splitting
                if relocation_record.section.sh_info == rodata_section_indices[0] as u32 {
                    if num_rodata_symbols == 1 {
                        continue; // nothing to do
                    }

                    // Split relocations across multiple .rodata sections
                    let mut new_relocations: Vec<Vec<Relocation>> =
                        vec![vec![]; rodata_section_indices.len()];

                    for mut relocation in relocation_record.relocations.clone() {
                        // Find which rodata section this relocation belongs to
                        for i in 0..rodata_section_offsets.len() {
                            if relocation.r_offset < rodata_section_offsets[i] as u32 {
                                if i > 0 {
                                    // Adjust offset relative to this section's start
                                    relocation.r_offset -= rodata_section_offsets[i - 1] as u32;
                                }
                                new_relocations[i].push(relocation);
                                break;
                            }
                        }
                    }

                    // Create new relocation records for each rodata section
                    for (i, relocations) in new_relocations.iter().enumerate() {
                        let mut new_rodata_reloc = if i == 0 {
                            relocation_record.section.clone()
                        } else {
                            relocation_section.clone()
                        };

                        new_rodata_reloc.sh_info = rodata_section_indices[i] as u32;

                        let mut new_reloc_record = RelocationRecord::new(new_rodata_reloc);
                        new_reloc_record.relocations = relocations.clone();
                        new_reloc_record.pack();
                        new_rodata_relocs.push(new_reloc_record.section.clone());

                        if i == 0 {
                            // the original relocation section needs to be updated here
                            compiled_elf.sections[idx] = new_reloc_record.section;
                        }
                    }

                    continue;
                }

                // Update symbol indices for other relocations
                for relocation in &mut relocation_record.relocations {
                    if relocation.symbol_index() >= initial_sh_info_value as usize {
                        relocation.set_symbol_index(
                            (relocation.symbol_index() + local_syms_inserted) as u32,
                        );
                    }
                }

                // Update the section in the ELF
                relocation_record.pack();
                let section_idx = compiled_elf
                    .sections
                    .iter()
                    .position(|s| {
                        s.sh_type == relocation_section.sh_type
                            && s.sh_info == relocation_section.sh_info
                            && s.sh_name == relocation_section.sh_name
                    })
                    .expect("relocation section not found");
                compiled_elf.sections[section_idx] = relocation_record.section;
            }

            // Add the new rodata relocation sections (skip first as it was amended in place)
            for new_rodata_reloc in new_rodata_relocs.into_iter().skip(1) {
                compiled_elf.add_section(new_rodata_reloc);
            }
        }

        for symbol in assembled_elf.get_symbols() {
            if symbol.st_name == 0 {
                continue; // Skip null symbol
            }

            if symbol.bind() == 0 {
                continue; // Ignore local symbols
            }

            // TODO: is the symbol text alread here?
            if !asm_text.is_empty() && !reloc_symbols.contains(&symbol.name) {
                let mut sym = symbol.clone();
                sym.st_shndx = text_section_index as u16;
                compiled_elf.add_symbol(sym);
            }
        }
    }

    write_dependency_file(compiler, make_rule, c_content, o_file)?;
    write_obj(o_file, &compiled_elf.pack())?;

    Ok(())
}

fn ensure_parent_dir<P: AsRef<Path>>(path: P) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.as_ref().parent() {
        if !parent.is_dir() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

pub fn write_obj<P: AsRef<Path>>(path: P, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    ensure_parent_dir(&path)?;
    std::fs::write(path.as_ref(), bytes)?;

    Ok(())
}

fn write_dependency_file<P: AsRef<Path>>(
    compiler: &Compiler,
    make_rule: Option<MakeRule>,
    c_content: &NamedString,
    o_file: P,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut make_rule) = make_rule else {
        return Ok(());
    };

    make_rule.target = o_file.as_ref().to_string_lossy().into_owned();

    // For stdin there is no meaningful source path to record.
    // For a real file, include it only when running interactively; piped
    // build systems (make, ninja) already know the source from their own rules.
    make_rule.source = if matches!(c_content.source, SourceType::StdIn) {
        None
    } else if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        Some(c_content.source.name().clone())
    } else {
        None
    };

    let d_file = if compiler.gcc_deps {
        // gcc mode: write the .d file alongside the output object as <name>.o.d
        let mut p = o_file.as_ref().to_path_buf();
        let new_ext = format!(
            "{}.d",
            o_file
                .as_ref()
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
        );
        p.set_extension(new_ext);
        p
    } else {
        // mw mode: write the .d file alongside the source .c file as <name>.d
        PathBuf::from(&c_content.source.name()).with_extension("d")
    };

    ensure_parent_dir(&d_file)?;
    std::fs::write(&d_file, make_rule.as_str().as_bytes())?;

    Ok(())
}
