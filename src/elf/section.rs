// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use std::fmt;
use std::io::{Cursor, Read};

use super::strtab::StrTab;
use super::symtab::SymTab;
use crate::le::read_u32_le;

// ELF section types (sh_type)
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_REL: u32 = 9;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Section {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u32,
    pub sh_addr: u32,
    pub sh_offset: u32,
    pub sh_size: u32,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u32,
    pub sh_entsize: u32,
    pub data: Vec<u8>,

    pub name: String,
}

impl Section {
    pub const HEADER_SIZE: usize = 40; // 10 fields * 4 bytes

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sh_name: u32,
        sh_type: u32,
        sh_flags: u32,
        sh_addr: u32,
        sh_offset: u32,
        sh_size: u32,
        sh_link: u32,
        sh_info: u32,
        sh_addralign: u32,
        sh_entsize: u32,
        data: Vec<u8>,
    ) -> Self {
        Self {
            sh_name,
            sh_type,
            sh_flags,
            sh_addr,
            sh_offset,
            sh_size,
            sh_link,
            sh_info,
            sh_addralign,
            sh_entsize,
            data,
            name: String::new(),
        }
    }

    pub fn pack_header(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::HEADER_SIZE);
        buf.extend_from_slice(&self.sh_name.to_le_bytes());
        buf.extend_from_slice(&self.sh_type.to_le_bytes());
        buf.extend_from_slice(&self.sh_flags.to_le_bytes());
        buf.extend_from_slice(&self.sh_addr.to_le_bytes());
        buf.extend_from_slice(&self.sh_offset.to_le_bytes());
        if self.sh_type != SHT_NOBITS {
            // Matches Python: self.sh_size is replaced by current data length
            buf.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        } else {
            buf.extend_from_slice(&(self.sh_size).to_le_bytes());
        }
        buf.extend_from_slice(&self.sh_link.to_le_bytes());
        buf.extend_from_slice(&self.sh_info.to_le_bytes());
        buf.extend_from_slice(&self.sh_addralign.to_le_bytes());
        buf.extend_from_slice(&self.sh_entsize.to_le_bytes());
        buf
    }

    pub fn pack_data(&self) -> Vec<u8> {
        self.data.clone()
    }

    pub fn pack(&self) -> (Vec<u8>, Vec<u8>) {
        (self.pack_header(), self.pack_data())
    }

    pub fn unpack(rdr: &mut Cursor<&[u8]>) -> Self {
        let sh_name = read_u32_le(rdr); // 1
        let sh_type = read_u32_le(rdr); // 2
        let sh_flags = read_u32_le(rdr); // 3
        let sh_addr = read_u32_le(rdr); // 4
        let sh_offset = read_u32_le(rdr); // 5
        let sh_size = read_u32_le(rdr);
        let sh_link = read_u32_le(rdr);
        let sh_info = read_u32_le(rdr);
        let sh_addralign = read_u32_le(rdr);
        let sh_entsize = read_u32_le(rdr);

        let mut data: Vec<u8> = vec![];
        if sh_type != SHT_NOBITS {
            data = vec![0u8; sh_size as usize];
            rdr.set_position(sh_offset as u64);
            rdr.read_exact(&mut data).unwrap();
        }

        Self {
            sh_name,
            sh_type,
            sh_flags,
            sh_addr,
            sh_offset,
            sh_size,
            sh_link,
            sh_info,
            sh_addralign,
            sh_entsize,
            data,
            name: String::new(), // Populated later
        }
    }
}

// Equivalent to __str__
impl fmt::Display for Section {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let size = if self.sh_type == SHT_NOBITS {
            self.sh_size
        } else {
            self.data.len() as u32
        };
        write!(
            f,
            "sh_name: 0x{:X} sh_type: 0x{:X} sh_flags: 0x{:X} sh_addr: 0x{:X} \
             sh_offset: 0x{:X} sh_size: 0x{:X} sh_link: 0x{:X} sh_info: 0x{:X} \
             sh_addralign: 0x{:X} sh_entsize: 0x{:X}",
            self.sh_name,
            self.sh_type,
            self.sh_flags,
            self.sh_addr,
            self.sh_offset,
            size,
            self.sh_link,
            self.sh_info,
            self.sh_addralign,
            self.sh_entsize
        )
    }
}

// Dummy TextSection for completeness - implementation depends on your existing logic
#[derive(Debug, Clone, Default)]
pub struct TextSection {
    pub section: Section,
    pub function_name: String,
}

impl TextSection {
    pub fn from_section(section: Section) -> Self {
        Self {
            section,
            function_name: String::new(),
        }
    }
    pub fn pack(&self) -> (Vec<u8>, Vec<u8>) {
        self.section.pack()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BssSection {
    pub section: Section,
}

impl BssSection {
    pub fn from_section(section: Section) -> Self {
        Self { section }
    }

    /// Equivalent to pack_data
    pub fn pack_data(&self) -> Vec<u8> {
        // SHT_NOBITS sections occupy no space in the file.
        // We return an empty vector regardless of what self.section.data contains.
        Vec::new()
    }

    /// Equivalent to pack
    pub fn pack(&self) -> (Vec<u8>, Vec<u8>) {
        let header = self.section.pack_header();
        let data = self.pack_data();
        (header, data)
    }
}

#[derive(Debug, Clone)]
pub enum SectionVariant {
    Raw(Section),
    SymTab(SymTab),
    StrTab(StrTab),
    Rel(RelocationRecord),
    Bss(BssSection),
    Text(TextSection),
}

impl SectionVariant {
    pub fn as_section_mut(&mut self) -> &mut Section {
        match self {
            SectionVariant::Raw(s) => s,
            SectionVariant::Bss(s) => &mut s.section,
            SectionVariant::SymTab(s) => &mut s.section,
            SectionVariant::StrTab(s) => &mut s.section,
            SectionVariant::Rel(s) => &mut s.section,
            SectionVariant::Text(s) => &mut s.section,
        }
    }

    pub fn pack(&mut self) -> (Vec<u8>, Vec<u8>) {
        match self {
            SectionVariant::Raw(s) => s.pack(),
            SectionVariant::Bss(s) => s.pack(),
            SectionVariant::SymTab(s) => s.pack(),
            SectionVariant::StrTab(s) => s.pack(),
            SectionVariant::Rel(s) => s.pack(),
            SectionVariant::Text(s) => s.pack(),
        }
    }

    pub fn wrap(section: Section) -> Self {
        match section.sh_type {
            SHT_SYMTAB => Self::SymTab(SymTab::new(section)),
            SHT_STRTAB => Self::StrTab(StrTab::new(section)),
            _ => Self::Raw(section),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Relocation {
    pub r_offset: u32,
    pub r_info: u32,
    pub symbol: String,
}

impl Relocation {
    pub fn symbol_index(&self) -> usize {
        // eprintln!("   get symbol index: {}", (self.r_info >> 8));
        (self.r_info >> 8) as usize
    }

    pub fn set_symbol_index(&mut self, index: u32) {
        if index > 0xFFFFFF {
            panic!(
                "Relocation cannot reference a symbols above {}, got {index}",
                0xFFFFFF
            );
        }
        // eprintln!("   set symbol index: {index}");
        self.r_info = (index << 8) | self.type_id()
    }

    pub fn type_id(&self) -> u32 {
        self.r_info & 0xff
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8);
        buf.extend_from_slice(&self.r_offset.to_le_bytes());
        buf.extend_from_slice(&self.r_info.to_le_bytes());
        buf
    }

    pub fn unpack(data: &[u8]) -> Self {
        let r_offset = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let r_info = u32::from_le_bytes(data[4..8].try_into().unwrap());
        Self {
            r_offset,
            r_info,
            symbol: String::new(),
        }
    }

    pub fn unpack_all(data: &[u8]) -> Vec<Self> {
        data.chunks(8).map(Relocation::unpack).collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RelocationRecord {
    pub section: Section,
    pub relocations: Vec<Relocation>,
}

impl RelocationRecord {
    pub fn new(section: Section) -> Self {
        // Equivalent to _handle_data: parse the section data into relocation records
        // Uses your Relocation::unpack_all implementation
        let relocations = Relocation::unpack_all(&section.data);

        Self {
            section,
            relocations,
        }
    }

    /// Equivalent to pack_data: serializes all relocations into the internal data buffer
    pub fn pack_data(&mut self) -> &[u8] {
        let mut buf = Vec::with_capacity(self.relocations.len() * 8); // 2 * u32 = 8 bytes per record
        for rel in &self.relocations {
            buf.extend_from_slice(&rel.pack());
        }
        // eprintln!("creating reloc data of size 0x{:x}", buf.len());
        self.section.sh_size = buf.len() as u32;
        self.section.data = buf;
        &self.section.data
    }

    /// Helper to pack both header and data
    pub fn pack(&mut self) -> (Vec<u8>, Vec<u8>) {
        let data = self.pack_data().to_vec();
        let header = self.section.pack_header();
        // eprintln!("packed reloc data of 0x{:x} (actual data len 0x{:x}", self.section.sh_size, self.section.data.len());
        (header, data)
    }

    /// Helper to add a relocation record
    pub fn add_relocation(&mut self, rel: Relocation) {
        self.relocations.push(rel);
    }
}
