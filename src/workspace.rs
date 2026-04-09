// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use std::path::{Path, PathBuf};

use tempfile::{Builder, TempDir};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TempMode {
    /// Use /dev/shm if available, else system temp. Always clean up.
    Normal,
    /// Use system temp. On failure, keep files and print their location.
    KeepOnFailure,
    /// Use /dev/shm if available, else system temp. Always clean up at
    /// termination regardless of outcome - files are never left orphaned.
    ShmDebug,
}

impl TempMode {
    fn base_dir(&self) -> PathBuf {
        let shm = Path::new("/dev/shm");
        match self {
            Self::Normal | Self::ShmDebug => {
                if shm.is_dir() {
                    shm.to_path_buf()
                } else {
                    std::env::temp_dir()
                }
            }
            Self::KeepOnFailure => std::env::temp_dir(),
        }
    }
}

pub struct Workspace {
    dir: TempDir,
    mode: TempMode,
}

impl Workspace {
    pub fn new(mode: TempMode) -> Result<Self, Box<dyn std::error::Error>> {
        let dir = Builder::new().prefix("mw-").tempdir_in(mode.base_dir())?;
        Ok(Self { dir, mode })
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Call when the operation fails. `KeepOnFailure` leaks the directory so
    /// it survives process exit; all other modes drop and clean up immediately.
    /// `ShmDebug` guarantees no orphaned files in /dev/shm.
    pub fn on_failure(self) {
        if self.mode == TempMode::KeepOnFailure {
            let path = self.dir.keep();
            eprintln!("note: temp files retained at {}", path.display());
        }
        // Normal / ShmDebug: dir drops here, deleting the workspace.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_normal_creates_dir() {
        let ws = Workspace::new(TempMode::Normal).unwrap();
        assert!(ws.path().is_dir());
    }

    #[test]
    fn test_workspace_cleans_up_on_drop() {
        let path = {
            let ws = Workspace::new(TempMode::Normal).unwrap();
            ws.path().to_path_buf()
        };
        assert!(!path.exists(), "workspace dir should be deleted on drop");
    }

    #[test]
    fn test_workspace_keep_on_failure_leaks() {
        let path = {
            let ws = Workspace::new(TempMode::KeepOnFailure).unwrap();
            let p = ws.path().to_path_buf();
            ws.on_failure();
            p
        };
        assert!(path.exists(), "KeepOnFailure should retain the dir");
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[test]
    fn test_workspace_shm_debug_cleans_up_on_failure() {
        let path = {
            let ws = Workspace::new(TempMode::ShmDebug).unwrap();
            let p = ws.path().to_path_buf();
            ws.on_failure();
            p
        };
        assert!(!path.exists(), "ShmDebug should always clean up");
    }
}
