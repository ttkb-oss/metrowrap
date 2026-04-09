// SPDX-FileCopyrightText: © 2025 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use crate::error::MWError;
use crate::makerule::{MakeRule, path_from_wibo};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;

pub struct Compiler {
    pub c_flags: Vec<String>,
    pub mwcc_path: PathBuf,
    pub use_wibo: bool,
    pub wibo_path: PathBuf,
    pub gcc_deps: bool,
}

impl Compiler {
    pub fn new(
        c_flags: Vec<String>,
        mwcc_path: PathBuf,
        use_wibo: bool,
        wibo_path: PathBuf,
    ) -> Self {
        let mut gcc_deps = false;
        for flag in &c_flags {
            if flag == "-gccdep" || flag == "-gccdepends" {
                gcc_deps = true;
            }
            if flag == "-nogccdep" || flag == "-nogccdepends" {
                gcc_deps = false;
            }
        }
        Self {
            c_flags,
            mwcc_path,
            use_wibo,
            wibo_path,
            gcc_deps,
        }
    }

    pub fn compile_file<P: AsRef<Path>>(
        &self,
        c_file: P,
        display_name: impl AsRef<str> + ToString,
        workspace: &Path,
    ) -> Result<(Vec<u8>, Option<MakeRule>), MWError> {
        let o_file = workspace.join("result.o");

        let mut cmd_args = vec!["-c".to_string()];
        cmd_args.extend(self.c_flags.clone());
        cmd_args.extend(vec![
            "-o".to_string(),
            o_file.to_string_lossy().to_string(),
            c_file.as_ref().to_string_lossy().to_string(),
        ]);

        let mut cmd = if self.use_wibo {
            let mut c = Command::new(&self.wibo_path);
            c.arg(&self.mwcc_path);
            c
        } else {
            Command::new(&self.mwcc_path)
        };

        cmd.args(cmd_args)
            .env("MWCIncludes", ".")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut process = cmd.spawn()?;

        // Read both stdout and stderr on background threads so neither pipe
        // buffer can fill and deadlock the other.
        let c_file_path = c_file.as_ref().to_path_buf();
        let display_name = display_name.to_string();

        let stdout_handle = {
            let stdout = process.stdout.take().unwrap();
            std::thread::spawn(move || -> std::io::Result<()> {
                // MWCC emits diagnostics on stdout; translate paths and
                // re-emit to our stderr so output streams stay coherent.
                for line in BufReader::new(stdout).lines() {
                    eprintln!(
                        "{}",
                        filter_diagnostic_line(&line?, &c_file_path, &display_name)
                    );
                }
                Ok(())
            })
        };

        let stderr_handle = {
            let stderr = process.stderr.take().unwrap();
            std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
                use std::io::Read;
                let mut buf = Vec::new();
                BufReader::new(stderr).read_to_end(&mut buf)?;
                Ok(buf)
            })
        };

        process.wait()?;

        stdout_handle.join().unwrap()?;

        // Forward any stderr from wibo / the OS (not from MWCC itself).
        let stderr_bytes = stderr_handle.join().unwrap()?;
        if !stderr_bytes.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&stderr_bytes));
        }

        if !o_file.exists() {
            // TODO: make saving temp file an option
            // std::mem::forget(temp_dir);

            return Err(MWError::Compiler(format!(
                "Compilation failed: {} {}",
                c_file.as_ref().to_string_lossy(),
                o_file.to_string_lossy(),
            )));
        }

        let obj_bytes = std::fs::read(&o_file)?;
        let make_rule = self.handle_dependency_file(c_file, workspace)?;

        Ok((obj_bytes, make_rule))
    }

    fn handle_dependency_file<P1: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        c_file: P1,
        temp_dir: P2,
    ) -> Result<Option<MakeRule>, MWError> {
        if self.gcc_deps {
            let d_file = temp_dir.as_ref().join("result.d");
            if d_file.exists() {
                return Ok(Some(
                    MakeRule::new(&std::fs::read(d_file)?, self.use_wibo).unwrap(),
                ));
            }
        } else if self.c_flags.iter().any(|f| f == "-MD" || f == "-MMD") {
            let d_file_name = c_file
                .as_ref()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .replace(".c", ".d");
            let d_file = Path::new(&d_file_name);
            if d_file.exists() {
                let data = std::fs::read(d_file)?;
                std::fs::remove_file(d_file)?;
                return Ok(Some(MakeRule::new(&data, self.use_wibo).unwrap()));
            }
        }
        Ok(None)
    }
}

/// Translates a single line of MWCC diagnostic output:
///
/// - `# In:` and `# From:` path fields have DOS backslashes converted to forward slashes
///   and wibo-style drive prefixes (`Z:\`, `//?/`) stripped via [`path_from_wibo`].
/// - `# From:` additionally replaces the temp file path we passed to the compiler with
///   the user-visible name (`display_name`, which may be `<stdin>`).
///
/// All other lines are returned unchanged.
pub fn filter_diagnostic_line(line: &str, temp_path: &Path, display_name: &str) -> String {
    // Both "# In:" and "# From:" carry a file path after the fixed-width prefix.
    // The prefix is exactly "#      In: " (11 chars) or "#    From: " (11 chars).
    let (tag, rest) = if let Some(r) = line.strip_prefix("#      In: ") {
        ("#      In: ", r)
    } else if let Some(r) = line.strip_prefix("#    From: ") {
        ("#    From: ", r)
    } else if let Some(r) = line.strip_prefix("#    File: ") {
        ("#    File: ", r)
    } else {
        return line.to_string();
    };

    // Convert the path to POSIX form.
    let posix = path_from_wibo(rest.trim_end());
    let posix_str = posix.to_string_lossy();

    if tag == "#    From: " || tag == "#    File: " {
        let matches = temp_path
            .canonicalize()
            .ok()
            .zip(posix.canonicalize().ok())
            .map(|(a, b)| a == b)
            .unwrap_or(false);

        // Replace the temp path with the user-visible name. We compare by
        // rendered match or canonical string because the path has already been through
        // path_from_wibo.
        let display = if matches || posix_str == temp_path.to_string_lossy() {
            display_name.to_string()
        } else {
            posix_str.into_owned()
        };
        format!("{tag}{display}")
    } else {
        format!("{tag}{posix_str}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_compiler_init() {
        let compiler = Compiler::new(
            vec!["-gccdep".to_string()],
            PathBuf::from("mwcc.exe"),
            false,
            PathBuf::from("wibo"),
        );
        assert!(compiler.gcc_deps);
    }

    #[test]
    fn test_compiler_flag_detection() {
        // Test that the compiler correctly identifies dependency modes from flags
        let compiler_gcc = Compiler::new(
            vec!["-gccdep".to_string()],
            PathBuf::from("mwcc.exe"),
            false,
            PathBuf::from("wibo"),
        );
        assert!(compiler_gcc.gcc_deps);

        let compiler_no_gcc = Compiler::new(
            vec!["-gccdep".to_string(), "-nogccdep".to_string()],
            PathBuf::from("mwcc.exe"),
            false,
            PathBuf::from("wibo"),
        );
        assert!(!compiler_no_gcc.gcc_deps);
    }

    #[test]
    fn test_filter_unrelated_line() {
        let line = "#   variable 'x' is not initialized before being used";
        let result = filter_diagnostic_line(line, Path::new("/tmp/foo.c"), "src/foo.c");
        assert_eq!(result, line);
    }

    #[test]
    fn test_filter_in_line_backslash() {
        // "# In:" paths only need backslash normalisation, not temp substitution.
        let line = "#      In: src\\st\\e_grave_keeper.h";
        let result = filter_diagnostic_line(line, Path::new("/tmp/foo.c"), "src/foo.c");
        assert_eq!(result, "#      In: src/st/e_grave_keeper.h");
    }

    #[test]
    fn test_filter_from_line_backslash() {
        // "# From:" with a non-temp path: only normalise slashes.
        let line = "#    From: src\\st\\are\\foo.c";
        let result = filter_diagnostic_line(line, Path::new("/tmp/.tmpXXXX.c"), "src/bar.c");
        assert_eq!(result, "#    From: src/st/are/foo.c");
    }

    #[test]
    fn test_filter_from_line_temp_substitution() {
        // "# From:" with the exact temp path: replace with the display name.
        let temp = Path::new("/tmp/.tmpJ8qy3h.c");
        let line = "#    From: /tmp/.tmpJ8qy3h.c";
        let result = filter_diagnostic_line(line, temp, "src/st/are/e_grave_keeper.c");
        assert_eq!(result, "#    From: src/st/are/e_grave_keeper.c");
    }

    #[test]
    fn test_filter_from_line_stdin() {
        let temp = Path::new("/tmp/.tmpABCDEF.c");
        let line = "#    From: /tmp/.tmpABCDEF.c";
        let result = filter_diagnostic_line(line, temp, "<stdin>");
        assert_eq!(result, "#    From: <stdin>");
    }

    #[test]
    fn test_filter_real_file() {
        let temp = Path::new("tests/data/assemlber.c");
        let line = "#    From: tests\\data\\assemlber.c";
        let result = filter_diagnostic_line(line, temp, "<stdin>");
        assert_eq!(result, "#    From: <stdin>");
    }

    #[test]
    fn test_filter_full_diagnostic_block() {
        // Simulate a complete MWCC diagnostic block end-to-end.
        let temp = Path::new("/tmp/.tmpJ8qy3h.c");
        let display = "src/st/are/e_grave_keeper.c";

        let input = [
            "### mwccpsp.exe Compiler:",
            "#      In: src\\st\\e_grave_keeper.h",
            "#    From: src\\st\\are\\.tmpJ8qy3h.c",
            "# --------------------------------",
            "#     135: s32 collisionDetected;",
            "# Warning:     ^^^^^^^^^^^^^^^^^",
            "#   variable 'collisionDetected' is not initialized before being used",
        ];

        // The temp path in this example is expressed as a relative DOS path, so
        // path_from_wibo will turn it into "src/st/are/.tmpJ8qy3h.c" — which
        // won't match our POSIX temp path.  This reflects a real edge case: when
        // MWCC runs under wibo it receives the POSIX path we passed, and the
        // diagnostic will echo that back verbatim rather than as a DOS path.
        // The test below uses a POSIX path in the From line to verify substitution.
        let input_posix = [
            "### mwccpsp.exe Compiler:",
            "#      In: src\\st\\e_grave_keeper.h",
            "#    From: /tmp/.tmpJ8qy3h.c",
            "# --------------------------------",
            "#     135: s32 collisionDetected;",
            "# Warning:     ^^^^^^^^^^^^^^^^^",
            "#   variable 'collisionDetected' is not initialized before being used",
        ];

        let expected = [
            "### mwccpsp.exe Compiler:",
            "#      In: src/st/e_grave_keeper.h",
            "#    From: src/st/are/e_grave_keeper.c",
            "# --------------------------------",
            "#     135: s32 collisionDetected;",
            "# Warning:     ^^^^^^^^^^^^^^^^^",
            "#   variable 'collisionDetected' is not initialized before being used",
        ];

        let _ = input; // kept for documentation; posix variant is what we test
        for (line, want) in input_posix.iter().zip(expected.iter()) {
            assert_eq!(&filter_diagnostic_line(line, temp, display), want);
        }

        // The temp path in this example is expressed as a relative DOS path, so
        // path_from_wibo will turn it into "src/st/are/.tmpJ8qy3h.c" — which
        // won't match our POSIX temp path.  This reflects a real edge case: when
        // MWCC runs under wibo it receives the POSIX path we passed, and the
        // diagnostic will echo that back verbatim rather than as a DOS path.
        // The test below uses a POSIX path in the From line to verify substitution.
        let input_posix = [
            "### mwccpsp.exe Compiler:",
            "#    File: src\\st\\e_grave_keeper.h",
            "# --------------------------------",
            "#     135: s32 collisionDetected;",
            "# Warning:     ^^^^^^^^^^^^^^^^^",
            "#   variable 'collisionDetected' is not initialized before being used",
        ];

        let expected = [
            "### mwccpsp.exe Compiler:",
            "#    File: src/st/e_grave_keeper.h",
            "# --------------------------------",
            "#     135: s32 collisionDetected;",
            "# Warning:     ^^^^^^^^^^^^^^^^^",
            "#   variable 'collisionDetected' is not initialized before being used",
        ];

        let _ = input; // kept for documentation; posix variant is what we test
        for (line, want) in input_posix.iter().zip(expected.iter()) {
            assert_eq!(&filter_diagnostic_line(line, temp, display), want);
        }
    }
}
