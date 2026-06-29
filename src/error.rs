// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MWError {
    #[error("Assembler error: {0}")]
    Assembler(String),

    #[error("Compiler error: {0}")]
    Compiler(String),

    #[error("Preprocessing error: {0}")]
    Preprocessor(String),

    #[error("ELF manipulation error: {0}")]
    Elf(String),

    #[error("Assembly file not found: {0}")]
    AsmFileNotFound(PathBuf),

    #[error("cannot execute '{program}': {source}")]
    ToolExecutionFailed {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Internal error: {0}")]
    Internal(&'static str),
}
