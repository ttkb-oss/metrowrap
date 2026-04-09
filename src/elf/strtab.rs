// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use memchr::memmem;

use super::Section;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StrTab {
    pub section: Section,
}

impl StrTab {
    pub fn new(section: Section) -> Self {
        Self { section } // ZERO COPY! No string parsing or Vec allocation.
    }

    pub fn pack_data(&mut self) -> &[u8] {
        // No-op! The data is already perfectly maintained in `self.section.data`
        &self.section.data
    }

    pub fn get_str(&self, index: usize) -> &str {
        let slice = &self.section.data[index..];
        let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
        std::str::from_utf8(&slice[..end]).unwrap_or("")
    }

    pub fn get_string(&self, index: usize) -> String {
        self.get_str(index).to_string()
    }

    /// Adds a symbol if it doesn't exist, otherwise returns the existing offset
    pub fn add_symbol(&mut self, symbol_name: &str) -> u32 {
        let needle = symbol_name.as_bytes();

        // 1. Handle the Empty String (Standard ELF convention: always index 0)
        if needle.is_empty() {
            if self.section.data.is_empty() {
                self.section.data.push(0);
                self.section.sh_size = 1;
            }
            return 0;
        }

        // 2. Optimized Search (memmem)
        let mut search_offset = 0;
        while let Some(sub_slice) = self.section.data.get(search_offset..) {
            let Some(found_idx) = memmem::find(sub_slice, needle) else {
                break;
            };
            let actual_idx = search_offset + found_idx;
            let end_idx = actual_idx + needle.len();

            // Ensure it's a full string match (followed by \0)
            if self.section.data.get(end_idx) == Some(&0) {
                return actual_idx as u32;
            }

            // Move forward to find the next occurrence
            search_offset = actual_idx + 1;
        }

        // 3. Not found: Append
        let idx = self.section.data.len();
        self.section.data.extend_from_slice(needle);
        self.section.data.push(0); // Null terminator
        self.section.sh_size = self.section.data.len() as u32;

        idx as u32
    }

    pub fn pack(&mut self) -> (Vec<u8>, Vec<u8>) {
        let data = self.section.data.clone();
        (self.section.pack_header(), data)
    }
}

#[cfg(test)]
mod test {
    use super::super::SHT_STRTAB;
    use super::Section;
    use super::*;

    #[test]
    fn test_add_symbol() {
        let section = Section::new(0, SHT_STRTAB, 0, 0, 0, 0, 0, 0, 0, 0, vec![]);
        let mut strtab = StrTab::new(section);

        let index = strtab.add_symbol("");
        assert_eq!(0, index);
        assert_eq!(vec![0], strtab.section.data);

        strtab.add_symbol(".rel.text");

        let (_header, data) = strtab.pack();
        assert_eq!("\0.rel.text\0".bytes().collect::<Vec<u8>>(), data);
    }
}
