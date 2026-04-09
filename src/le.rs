// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE

use std::io::{Cursor, Read};

pub fn read_u32_le(rdr: &mut Cursor<&[u8]>) -> u32 {
    let mut b = [0u8; 4];
    rdr.read_exact(&mut b).unwrap();
    u32::from_le_bytes(b)
}

pub fn read_u16_le(rdr: &mut Cursor<&[u8]>) -> u16 {
    let mut b = [0u8; 2];
    rdr.read_exact(&mut b).unwrap();
    u16::from_le_bytes(b)
}
