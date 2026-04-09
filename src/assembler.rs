// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use crate::error::MWError;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::Builder;

pub struct Assembler {
    pub as_path: String,
    pub as_march: String,
    pub as_mabi: String,
    pub as_flags: Vec<String>,
    pub macro_inc_path: Option<PathBuf>,
}

impl Assembler {
    pub fn assemble_file<P: AsRef<Path>>(
        &self,
        asm_filepath: P,
        workspace: &Path,
    ) -> Result<Vec<u8>, MWError> {
        self.assemble_data(File::open(asm_filepath)?, workspace)
    }

    pub fn assemble_data<R: Read>(
        &self,
        mut asm_data: R,
        workspace: &Path,
    ) -> Result<Vec<u8>, MWError> {
        let temp_o = Builder::new().suffix(".o").tempfile_in(workspace)?;

        let mut cmd = Command::new(&self.as_path);
        cmd.args([
            "-EL",
            &format!("-march={}", self.as_march),
            &format!("-mabi={}", self.as_mabi),
        ])
        .arg("-o")
        .arg(temp_o.path().to_str().unwrap())
        .args(&self.as_flags);

        if let Some(inc) = &self.macro_inc_path
            && let Some(parent) = inc.parent()
            && !parent.as_os_str().is_empty()
        {
            cmd.arg(format!("-I{}", parent.display()));
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut process = cmd.spawn()?;
        let mut stdin = process.stdin.take().unwrap();

        if let Some(include) = &self.macro_inc_path {
            if include.is_file() {
                std::io::copy(&mut File::open(include)?, &mut stdin)?;
            }
        }
        std::io::copy(&mut asm_data, &mut stdin)?;
        drop(stdin);

        let output = process.wait_with_output()?;

        if !output.status.success() {
            return Err(MWError::Assembler(
                String::from_utf8_lossy(&output.stderr).into(),
            ));
        }

        let mut obj_bytes = Vec::new();
        temp_o.reopen()?.read_to_end(&mut obj_bytes)?;

        Ok(obj_bytes)
    }
}
