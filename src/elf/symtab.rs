// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use std::collections::HashMap;

use super::Section;
use super::StrTab;
use super::Symbol;

use crate::constants::FUNCTION_PREFIX;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SymTab {
    pub section: Section,
    pub symbols: Vec<Symbol>,
    pub name_to_index: HashMap<String, usize>,
}

impl SymTab {
    pub fn new(section: Section) -> Self {
        let symbols = Symbol::unpack_all(&section.data);
        Self {
            section,
            symbols,
            name_to_index: HashMap::new(),
        }
    }

    pub fn get_symbol_index(&self, name: &str) -> Option<usize> {
        self.name_to_index.get(name).copied()
    }

    pub fn get_symbol_by_name(&self, name: &str) -> Option<(usize, &Symbol)> {
        self.get_symbol_index(name)
            .map(|idx| (idx, &self.symbols[idx]))
    }

    pub fn add_symbol(&mut self, symbol: Symbol) -> usize {
        let is_local = symbol.bind() == 0;
        let name = symbol.name.clone();

        let index = if is_local {
            let index = self.section.sh_info as usize;
            if index <= self.symbols.len() {
                self.symbols.insert(index, symbol);
                self.section.sh_info += 1;

                // Array shifted right! We must repair the cache for shifted elements.
                for i in index..self.symbols.len() {
                    if !self.symbols[i].name.is_empty() {
                        self.name_to_index.insert(self.symbols[i].name.clone(), i);
                    }
                }
                index
            } else {
                let idx = self.symbols.len();
                self.symbols.push(symbol);
                idx
            }
        } else {
            // Global/Weak symbols append to the end. No shifting required!
            let idx = self.symbols.len();
            self.symbols.push(symbol);
            idx
        };

        if !name.is_empty() {
            self.name_to_index.insert(name, index);
        }

        index
    }

    pub fn pack_data(&mut self) -> &[u8] {
        let mut buf = Vec::with_capacity(self.symbols.len() * 16);
        for sym in &self.symbols {
            buf.extend_from_slice(&sym.pack());
        }
        self.section.sh_size = buf.len() as u32;
        self.section.data = buf;
        &self.section.data
    }

    pub fn pack(&mut self) -> (Vec<u8>, Vec<u8>) {
        let data = self.pack_data().to_vec();
        let header = self.section.pack_header();
        (header, data)
    }

    pub fn pack_section(&mut self) -> Section {
        self.pack();
        self.section.clone()
    }

    pub fn populate_symbols(&mut self, strtab: &StrTab) {
        self.name_to_index.clear();
        for (i, sym) in self.symbols.iter_mut().enumerate() {
            sym.name = strtab.get_string(sym.st_name as usize);
            if !sym.name.is_empty() {
                self.name_to_index.insert(sym.name.clone(), i);
            }
        }
    }

    pub fn remove_function_prefix(&mut self) {
        let mut updates = Vec::new();
        for (i, sym) in self.symbols.iter_mut().enumerate() {
            if sym.name.starts_with(FUNCTION_PREFIX) {
                let old_name = sym.name.clone();
                sym.name = sym.name[FUNCTION_PREFIX.len()..].to_string();
                sym.st_name += FUNCTION_PREFIX.len() as u32;
                updates.push((old_name, sym.name.clone(), i));
            }
        }
        // Update the cache so lookups don't break!
        for (old, new, idx) in updates {
            self.name_to_index.remove(&old);
            self.name_to_index.insert(new, idx);
        }
        self.pack();
    }

    pub fn list(&self) -> Vec<String> {
        self.symbols.iter().map(|s| s.name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::SHT_SYMTAB;
    use super::Symbol;
    use super::*;
    use crate::elf::STB_GLOBAL;
    use crate::elf::STB_LOCAL;

    fn new_empty_section() -> Section {
        Section::new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, vec![])
    }

    fn symbol_from_string(name: u32, text: impl Into<String>) -> Symbol {
        Symbol {
            st_name: name,
            st_value: 0,
            st_size: 0,
            st_info: 0,
            st_other: 0,
            st_shndx: 0,
            name: text.into().clone(),
        }
    }

    #[test]
    fn test_roundtrip() {
        let mut empty_section = new_empty_section();
        empty_section.sh_type = SHT_SYMTAB;

        let mut symtab = SymTab::new(empty_section.clone());
        assert!(symtab.symbols.is_empty());

        let (header, _data) = symtab.pack();
        assert_eq!(empty_section.pack_header(), header);
    }

    #[test]
    fn test_add() {
        let mut symtab = SymTab::new(new_empty_section());
        symtab.section.sh_type = SHT_SYMTAB;

        let mut sym = symbol_from_string(7, "hello");
        sym.st_info = (STB_GLOBAL << 4) | (sym.st_info & 0xF);
        let index = symtab.add_symbol(sym);
        assert_eq!(0, index);
        assert_eq!(1, symtab.symbols.len());
        symtab.pack();
        assert_eq!(16, symtab.section.data.len());

        let mut sym = symbol_from_string(2, "world");
        sym.st_info = STB_GLOBAL << 4 | sym.st_info;
        let index = symtab.add_symbol(sym);
        assert_eq!(1, index);
        assert_eq!(2, symtab.symbols.len());
        symtab.pack();
        assert_eq!(32, symtab.section.data.len());

        let mut local_sym = symbol_from_string(8, "goodbye");
        local_sym.st_info = (STB_LOCAL << 4) | (local_sym.st_info & 0xF);
        let index = symtab.add_symbol(local_sym);
        assert_eq!(0, index);
        assert_eq!(3, symtab.symbols.len());
        symtab.pack();
        assert_eq!(48, symtab.section.data.len());
    }
}
