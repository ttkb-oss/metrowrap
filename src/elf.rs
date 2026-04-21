// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use std::cmp::max;
use std::io::{Cursor, Read, Write};

pub mod section;
pub mod strtab;
pub mod symbol;
pub mod symtab;

use crate::constants::DOLLAR_SIGN;
use crate::constants::SYMBOL_AT;
use crate::constants::SYMBOL_DOLLAR;

pub use crate::elf::section::Relocation;
pub use crate::elf::section::RelocationRecord;
pub use crate::elf::section::Section;
pub use crate::elf::section::SectionVariant;
pub use crate::elf::section::TextSection;
pub use crate::elf::strtab::StrTab;
pub use crate::elf::symbol::Symbol;
pub use crate::elf::symtab::SymTab;

use crate::le::read_u16_le;
use crate::le::read_u32_le;

pub use crate::elf::section::SHT_NOBITS;
pub use crate::elf::section::SHT_REL;
pub use crate::elf::section::SHT_RELA;
pub use crate::elf::section::SHT_STRTAB;
pub use crate::elf::section::SHT_SYMTAB;

// ELF symbol binding (st_info >> 4)
pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;

// ELF symbol type (st_info & 0xf)
pub const STT_NOTYPE: u8 = 0;
pub const STT_FUNC: u8 = 2;
pub const STT_SECTION: u8 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct ElfHeader {
    pub ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u32,
    pub e_phoff: u32,
    pub e_shoff: u32,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[derive(Debug)]
pub struct Elf {
    pub header: ElfHeader,
    pub sections: Vec<Section>,

    pub shstrtab_idx: usize,
    pub strtab_idx: usize,
    pub symtab_idx: usize,

    pub shstrtab: StrTab,
    pub strtab: StrTab,
    pub symtab: SymTab,
}

impl Elf {
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut rdr = Cursor::new(data);

        let mut ident = [0u8; 16];
        rdr.read_exact(&mut ident).unwrap();

        let header = ElfHeader {
            ident,
            e_type: read_u16_le(&mut rdr),
            e_machine: read_u16_le(&mut rdr),
            e_version: read_u32_le(&mut rdr),
            e_entry: read_u32_le(&mut rdr),
            e_phoff: read_u32_le(&mut rdr),
            e_shoff: read_u32_le(&mut rdr),
            e_flags: read_u32_le(&mut rdr),
            e_ehsize: read_u16_le(&mut rdr),
            e_phentsize: read_u16_le(&mut rdr),
            e_phnum: read_u16_le(&mut rdr),
            e_shentsize: read_u16_le(&mut rdr),
            e_shnum: read_u16_le(&mut rdr),
            e_shstrndx: read_u16_le(&mut rdr),
        };

        let shstrtab: StrTab = {
            rdr.set_position(
                (header.e_shoff + (header.e_shstrndx as u32 * header.e_shentsize as u32)) as u64,
            );
            let section = Section::unpack(&mut rdr);
            StrTab::new(section.clone())
        };

        let sections: Vec<Section> = (0..header.e_shnum)
            .map(|i| {
                rdr.set_position((header.e_shoff + (i as u32 * header.e_shentsize as u32)) as u64);

                let mut section = Section::unpack(&mut rdr);
                section.name = shstrtab.get_string(section.sh_name as usize);
                section
            })
            .collect();

        let symtab_idx = sections
            .iter()
            .position(|s| s.sh_type == SHT_SYMTAB)
            .expect("symtab");
        let shstrtab_idx = header.e_shstrndx as usize;
        let strtab_idx = sections
            .iter()
            .enumerate()
            .filter(|(i, s)| s.sh_type == SHT_STRTAB && *i as u16 != header.e_shstrndx)
            .map(|(i, _)| i)
            .next()
            .expect("strtab");

        let strtab = StrTab::new(sections[strtab_idx].clone());
        let mut symtab = SymTab::new(sections[symtab_idx].clone());
        symtab.populate_symbols(&strtab);

        Self {
            header,
            sections,
            shstrtab_idx,
            symtab_idx,
            strtab_idx,
            shstrtab,
            strtab,
            symtab,
        }
    }

    pub fn pack(&mut self) -> Vec<u8> {
        self.shstrtab.pack_data();
        self.strtab.pack_data();
        self.symtab.pack_data();
        self.sections[self.shstrtab_idx] = self.shstrtab.section.clone();
        self.sections[self.strtab_idx] = self.strtab.section.clone();
        self.sections[self.symtab_idx] = self.symtab.section.clone();

        let elf_header_size: u32 = 0x40;

        let mut out = Vec::new();

        let mut sh_offset = 0x34 + 0xC;

        let mut section_headers: Vec<u8> = vec![];
        let mut section_data: Vec<u8> = vec![];

        //   [Nr] Name              Type            Addr     Off    Size   ES Flg Lk Inf Al
        //   [ 0]                   NULL            00000000 000000 000000 00      0   0  0
        //   [ 1] .text             PROGBITS        00000000 000040 000010 00  AX  0   0 16
        //   [ 2] .data             PROGBITS        00000000 000050 000000 00  WA  0   0 16
        //   [ 3] .bss              NOBITS          00000000 000050 000000 00  WA  0   0 16
        //   [ 4] .reginfo          MIPS_REGINFO    00000000 000050 000018 18   A  0   0  4
        //   [ 5] .MIPS.abiflags    MIPS_ABIFLAGS   00000000 000068 000018 18   A  0   0  8
        //   [ 6] .pdr              PROGBITS        00000000 000080 000000 00      0   0  4
        //   [ 7] .gnu.attributes   GNU_ATTRIBUTES  00000000 000080 000010 00      0   0  1
        //   [ 8] .symtab           SYMTAB          00000000 000090 000090 10      9   8  4
        //   [ 9] .strtab           STRTAB          00000000 000120 000005 00      0   0  1
        //   [10] .shstrtab         STRTAB          00000000 000125 000059 00      0   0  1
        //         sh_offset: 112 .symtab
        // sh_offset: 145 .strtab
        // sh_offset: 217 .shstrtab
        // sh_offset: 244 .comment
        // sh_offset: 272 .text
        // sh_offset: 288 .mwcats
        // sh_offset: 296 .rel.mwcats

        for section in &mut self.sections {
            section.sh_offset = sh_offset;
            let data = section.pack_data();
            let len = data.len() as u32;

            let header = section.pack_header();

            sh_offset += len;

            if section.sh_type != SHT_NOBITS {
                assert_eq!(
                    section.sh_size, len,
                    "section {} has wrong size: {section}",
                    section.name
                );
            }

            section_headers.extend(header);
            section_data.extend(data);

            let alignment = 1 << max(0, section.sh_addralign);
            if (sh_offset % alignment) != 0 {
                let bytes_needed = alignment - (sh_offset % alignment);
                section_data.extend(vec![0u8; bytes_needed as usize]);
                sh_offset += bytes_needed;
            }
        }

        if (sh_offset % 4) != 0 {
            let bytes_needed = 4 - (sh_offset % 4);
            section_data.extend(vec![0u8; bytes_needed as usize]);
            // sh_offset += bytes_needed;
        }

        out.resize(52, 0);
        let mut header_cursor = Cursor::new(&mut out[0..52]);
        let h = &self.header;

        header_cursor.write_all(&h.ident).unwrap();
        header_cursor.write_all(&h.e_type.to_le_bytes()).unwrap();
        header_cursor.write_all(&h.e_machine.to_le_bytes()).unwrap();
        header_cursor.write_all(&h.e_version.to_le_bytes()).unwrap();
        header_cursor.write_all(&h.e_entry.to_le_bytes()).unwrap();
        header_cursor.write_all(&h.e_phoff.to_le_bytes()).unwrap();
        header_cursor
            .write_all(&(elf_header_size + section_data.len() as u32).to_le_bytes())
            .unwrap(); // e_shoff
        header_cursor.write_all(&h.e_flags.to_le_bytes()).unwrap();
        header_cursor.write_all(&h.e_ehsize.to_le_bytes()).unwrap();
        header_cursor
            .write_all(&h.e_phentsize.to_le_bytes())
            .unwrap();
        header_cursor.write_all(&h.e_phnum.to_le_bytes()).unwrap();
        header_cursor
            .write_all(&h.e_shentsize.to_le_bytes())
            .unwrap();
        header_cursor
            .write_all(&(self.sections.len() as u16).to_le_bytes())
            .unwrap(); // e_shnum
        header_cursor
            .write_all(&h.e_shstrndx.to_le_bytes())
            .unwrap();
        out.extend([0u8; 0xC]);

        out.extend(section_data);
        out.extend(section_headers);

        out
    }

    pub fn symtab(&self) -> &SymTab {
        &self.symtab
    }

    pub fn set_symtab(&mut self, symtab: &SymTab) {
        self.symtab = symtab.clone();
    }

    pub fn get_symbols(&self) -> &Vec<Symbol> {
        &self.symtab().symbols
    }

    pub fn get_symbol_by_name(&self, name: String) -> (usize, Symbol) {
        self.get_symbols()
            .iter()
            .enumerate()
            .find(|(_, s)| s.name == name)
            .map(|(i, s)| (i, s.clone()))
            .expect("symbol")
    }

    pub fn find_section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    pub fn find_symbol(&self, name: &str) -> Option<Symbol> {
        self.get_symbols().iter().find(|s| s.name == name).cloned()
    }

    pub fn add_section(&mut self, mut section: Section) -> usize {
        section.sh_name = self.add_sh_symbol(&section.name);
        self.sections.push(section);
        self.sections.len() - 1
    }

    /// Adds a symbol to the ELF's symbol table and its name to the string table. Return the index.
    pub fn add_symbol(&mut self, sym: Symbol) -> usize {
        self.add_symbol_get_index(sym, false)
    }

    pub fn add_symbol_get_index(&mut self, symbol: Symbol, force: bool) -> usize {
        if !force {
            // Check if symbol already exists
            let existing_index = self
                .symtab
                .symbols
                .iter()
                .position(|s| s.name == symbol.name);

            if let Some(index) = existing_index {
                return index; // Symbol exists and we're not forcing
            }
        }

        let mut sym = symbol.clone();

        // Symbol doesn't exist or we're forcing - add it
        if !sym.name.is_empty() {
            sym.st_name = self.strtab.add_symbol(&symbol.name);
        }

        self.symtab.add_symbol(sym)
    }

    pub fn function_names(&self) -> Vec<&str> {
        let mut function_symbols: Vec<&Symbol> = self
            .get_symbols()
            .iter()
            .filter(|sym| sym.type_id() == STT_FUNC)
            .collect();

        function_symbols.sort_by_key(|s| s.st_shndx);

        function_symbols
            .into_iter()
            .map(|sym| sym.name.as_str())
            .collect()
    }

    pub fn get_functions(&self) -> Vec<TextSection> {
        let function_names = self.function_names();

        self.sections
            .iter()
            .filter(|s| s.name == ".text" || s.name.starts_with(".text"))
            .enumerate()
            .map(|(i, s)| {
                let mut text = TextSection::from_section(s.clone());
                if !function_names.is_empty() {
                    text.function_name = function_names[i].to_string();
                }
                text
            })
            .collect()
    }

    pub fn text_section_by_name(&self, name: impl Into<String>) -> usize {
        let name = name.into();
        let function_names = self.function_names();
        self.sections
            .iter()
            .enumerate()
            .filter(|(_, section)| section.name == ".text" || section.name.starts_with(".text."))
            .enumerate()
            .filter(|(f, _)| name == function_names[*f])
            .map(|(_, (i, _))| i)
            .next()
            .expect("function text index")
    }

    pub fn rodata_sections(&self) -> Vec<&Section> {
        self.sections
            .iter()
            .filter(|s| s.name == ".rodata" || s.name.starts_with(".rodata."))
            .collect()
    }

    pub fn relocation_sections(&self) -> Vec<RelocationRecord> {
        self.sections
            .iter()
            .filter(|s| s.sh_type == SHT_REL)
            .map(|s| {
                let mut rr = RelocationRecord::new(s.clone());
                for reloc in &mut rr.relocations {
                    let sym_index = reloc.symbol_index();
                    let sym = &self.symtab.symbols[sym_index];
                    reloc.symbol = sym.name.clone();
                }
                rr
            })
            .collect()
    }

    pub fn reloc_sections(&self) -> Vec<(usize, Section)> {
        self.sections
            .iter()
            .enumerate()
            .filter(|(_, s)| s.sh_type == SHT_REL)
            .map(|(i, s)| (i, s.clone()))
            .collect()
    }

    // TODO: rename: add_sh_str
    pub fn add_sh_symbol(&mut self, symbol_name: impl AsRef<str>) -> u32 {
        self.shstrtab.add_symbol(symbol_name.as_ref())
    }

    // TODO: this is lib business logic
    pub fn symbol_cleanup(&mut self) {
        // no symbols need to change here
        // remove the function prefix
        self.symtab.remove_function_prefix();

        // replace the __at__ prefix with @
        for sym in self.get_symbols().clone() {
            if sym.name.starts_with(SYMBOL_AT) {
                let mut sym = sym.clone();
                sym.name = "@".to_owned() + &sym.name[1..];
                self.add_symbol(sym);
            }
        }

        // replace all __dollar__ with $
        for sym in self.get_symbols().clone() {
            if sym.name.contains(SYMBOL_DOLLAR) {
                let mut sym = sym.clone();
                sym.name = sym.name.replace(SYMBOL_DOLLAR, DOLLAR_SIGN);
                self.add_symbol(sym);
            }
        }
    }
}
