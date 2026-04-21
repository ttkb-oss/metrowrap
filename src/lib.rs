// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
pub mod assembler;
pub mod compiler;
pub mod constants;
pub mod elf;
pub mod error;
pub mod le;
pub mod makerule;
pub mod preprocessor;
pub mod workspace;

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use tempfile::Builder;

use crate::assembler::Assembler;
use crate::compiler::Compiler;
use crate::constants::*;
use crate::elf::Elf;
use crate::elf::STB_LOCAL;
use crate::elf::Section;
use crate::elf::section::{Relocation, RelocationRecord};
use crate::makerule::MakeRule;
use crate::preprocessor::Preprocessor;
use crate::workspace::Workspace;

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

pub struct NamedSource {
    pub source: SourceType,
    pub content: Vec<u8>,
    pub src_dir: PathBuf,
}

impl NamedSource {
    fn with_tmp<F, T>(&self, dir: &Path, f: F) -> Result<T, Box<dyn Error>>
    where
        F: FnOnce(&Path) -> Result<T, Box<dyn Error>>,
    {
        let c_file = Builder::new().suffix(".c").tempfile_in(dir)?;
        std::fs::write(c_file.path(), &self.content)?;
        let r = f(c_file.path());
        if let Err(e) = r {
            eprintln!(
                "Error occurred, temporary file available at {}",
                c_file.path().display()
            );
            return Err(e);
        }
        r
    }
}

/// Extracts rodata symbol info from an assembled ELF in source order.
///
/// Returns `(name, size_in_bytes, is_local)` for each symbol whose section
/// is `.rodata`, filtered to those with a non-empty name and non-zero size
/// (i.e. real data symbols, not section markers). Sorted by section index,
/// which equals source order because each `dlabel` gets its own
/// `.section .rodata` block.
fn rodata_symbols_from_elf(elf: &Elf) -> Vec<(String, usize, bool)> {
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
}

struct ASMObject<'a> {
    path: PathBuf,
    main_symbol: &'a str,
    all_symbols: Vec<(String, usize, bool)>,
    elf: Elf,
}

pub fn process_c_file(
    c_content: &NamedSource,
    o_file: &Path,
    preprocessor: &Preprocessor,
    compiler: &Compiler,
    assembler: &Assembler,
    workspace: &Workspace,
    skip_asm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let ws = workspace.path();

    // 1. Scan source for INCLUDE_ASM/INCLUDE_RODATA macros.
    //    Returns content split into segments around each macro call, and the
    //    ordered list of (asm_path, func_name) pairs. No .s files are read here.
    let (segments, asm_refs) = preprocessor.find_macro_refs(&c_content.content);

    if asm_refs.is_empty() {
        // No INCLUDE_ASM macros: compile the original file directly and we're done.
        let (obj_bytes, make_rule) = c_content.with_tmp(ws, |c_file| {
            Ok(compiler.compile_file(c_file, c_content.source.name(), ws)?)
        })?;
        write_dependency_file(compiler, make_rule, c_content, o_file)?;
        return write_obj(o_file, &obj_bytes);
    }

    let jobs: usize = 4;

    // 2. Assemble all .s files by spawning assembler processes in chunks of
    //    `jobs` at a time. Each chunk runs concurrently at the OS level; we
    //    collect results before spawning the next chunk.
    let mut asm_objects: Vec<ASMObject> = Vec::with_capacity(asm_refs.len());
    for chunk in asm_refs.chunks(jobs.max(1)) {
        let spawned: Vec<_> = chunk
            .iter()
            .map(|(asm_file, _)| {
                assembler
                    .spawn_file(asm_file, ws)
                    .inspect_err(|_err| eprintln!("Failed to assemble {}", asm_file.display()))
                    .expect("spawned assembly")
            })
            .collect();

        for (spawned_asm, (asm_file, func_name)) in spawned.into_iter().zip(chunk) {
            let assembled_bytes = spawned_asm.collect().expect("assembled bytes");
            let elf = Elf::from_bytes(&assembled_bytes);
            let rodata_syms = rodata_symbols_from_elf(&elf);
            asm_objects.push(ASMObject {
                path: asm_file.clone(),
                main_symbol: func_name,
                all_symbols: rodata_syms,
                elf,
            });
        }
    }

    // 3. Build the stub C source by interleaving the original content segments
    //    with the generated stubs. Each stub is derived entirely from the
    //    assembled ELF, not from parsing the .s source.
    let mut temp_c = Builder::new()
        .suffix(".c")
        .tempfile_in(&c_content.src_dir)?; // TODO: try to move into workspace

    for (i, asm_object) in asm_objects.iter().enumerate() {
        temp_c.write_all(segments[i])?;
        let text_byte_count = asm_object
            .elf
            .get_functions()
            .into_iter()
            .next()
            .map(|f| f.section.data.len())
            .unwrap_or(0);
        temp_c.write_all(&Preprocessor::stub_for(
            asm_object.main_symbol,
            text_byte_count,
            &asm_object.all_symbols,
        ))?;
    }
    temp_c.write_all(segments[asm_objects.len()])?;
    temp_c.flush()?;

    // 4. Compile the stub C source.
    let (recompiled_bytes, make_rule) =
        compiler.compile_file(temp_c.path(), c_content.source.name(), ws)?;
    let mut compiled_elf = Elf::from_bytes(&recompiled_bytes);

    let rel_text_sh_name = compiled_elf.add_sh_symbol(".rel.text");

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

    if skip_asm {
        write_dependency_file(compiler, make_rule, c_content, o_file)?;
        return write_obj(o_file, &compiled_elf.pack());
    }

    for mut asm_object /*(asm_file, main_symbol, rodata_syms, mut assembled_elf)*/ in asm_objects {
        let num_rodata_symbols = asm_object.all_symbols.len();

        // Extract text bytes from the assembled ELF. INCLUDE_RODATA files have
        // no function symbol and no text, so this yields an empty vec.
        let asm_functions = asm_object.elf.get_functions();
        assert!(
            asm_functions.len() <= 1,
            "{}: expected at most 1 function, found {}",
            asm_object.path.display(),
            asm_functions.len()
        );
        let asm_text_owned: Vec<u8> = asm_functions
            .into_iter()
            .next()
            .map(|f| f.section.data)
            .unwrap_or_default();
        let asm_text = &asm_text_owned;

        // if this is a function and that function is not an INCLUDE_ASM, ignore
        if !asm_text.is_empty() && !stub_functions.contains(asm_object.main_symbol) {
            continue;
        }

        let mut rodata_section_indices: Vec<usize> = vec![];
        let mut text_section_index: usize = 0xFFFFFFFF;

        if !asm_text.is_empty() {
            text_section_index = compiled_elf.text_section_by_name(asm_object.main_symbol);

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
                "Not enough assembly to fill {} in {}",
                asm_object.main_symbol,
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
            let idx = symbol_to_section_idx[asm_object.main_symbol];
            rodata_section_indices.push(idx.into());
        }

        let mut rodata_section_offsets: Vec<usize> = vec![];

        let rel_rodata_sh_name = if num_rodata_symbols > 0 {
            let rodata_sections = asm_object.elf.rodata_sections();
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

        let relocation_records = asm_object.elf.reloc_sections();
        assert!(
            relocation_records.len() < 3,
            "{} has too many relocation records",
            asm_object.path.display()
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

            let mut assembled_symtab = asm_object.elf.symtab().clone();
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
            asm_object.elf.set_symtab(&assembled_symtab);
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

        for symbol in asm_object.elf.get_symbols() {
            if symbol.st_name == 0 {
                continue; // Skip null symbol
            }

            if symbol.bind() == 0 {
                continue; // Ignore local symbols
            }

            // TODO: is the symbol text already here?
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
    c_content: &NamedSource,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_symbol() {
        assert_eq!(escape_symbol("foo.bar$baz"), "foo__at__bar__dollar__baz");
    }

    #[test]
    fn test_unescape_symbol() {
        assert_eq!(unescape_symbol("foo__at__bar__dollar__baz"), "foo.bar$baz");
    }

    #[test]
    fn test_roundtrip_symbol() {
        assert_eq!(
            unescape_symbol(&escape_symbol("foo.bar$baz")),
            "foo.bar$baz"
        );
    }
}
