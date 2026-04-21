// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use std::error::Error;
use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;

use clap::Parser;

use metrowrap::NamedSource;
use metrowrap::SourceType;
use metrowrap::assembler;
use metrowrap::compiler;
use metrowrap::preprocessor;
use metrowrap::workspace::{TempMode, Workspace};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "An advanced Metrowerks compiler wrapper for seamless inline assembly injection.",
    long_about = "\
metrowrap is a modern, high-performance drop-in replacement for legacy wrappers like mwccgap, designed to accelerate decompilation projects (like sotn-decomp). 

It seamlessly intercepts compilation requests, transparently extracting `INCLUDE_ASM` and `INCLUDE_RODATA` macros from C/C++ source files. It coordinates assembling these extracted blocks via modern GNU assemblers while compiling the C source using the legacy Metrowerks C Compiler (mwcc). Finally, it injects the resulting disassembled code directly into the compiled object payloads.

By utilizing cross-platform compatibility layers like wine or wibo, metrowrap completely abstracts away path translations, dependency files, and temp-file management, making it painless to maintain mixed C/Assembly codebases.",
    after_long_help = "\
EXAMPLES:
    Basic compilation of a C file:
        mw -o build/src/main.o -O2 -c src/main.c

    Compiling using wibo for increased performance, passing specific
    architecture flags to the GNU assembler:
        mw --use-wibo --as-march=allegrex --as-mabi=32 -o build/src/game.o -g src/game.c

    Passing a global macro include file to all extracted assembly blocks:
        mw --macro-inc-path=include/macro.inc -o build/src/math.o -O2 src/math.c

EXIT STATUS:
    0   Successful compilation and assembly without errors.
    1   A compilation error occurred or metrowrap encountered a fatal error.

BUG REPORTING:
    If you encounter bugs, especially related to differences in behavior from mwccgap, please report them to the project maintainers via the issue tracker."
)]
#[command(allow_external_subcommands = true)]
#[command(override_usage = "mw [OPTIONS]… -o <output> [COMPILER_FLAGS]… <file>")]
#[command(
    name = "mw",
    help_template = "\
{before-help}{name} {version}
{author-with-newline}{about-with-newline}
Usage: {usage}

Arguments:
  [OPTIONS]...         {name} options (described below)
  [COMPILER_FLAGS]...  Flags passed directly to the compiler
  <infile>             The input file to process (or '-' for stdin)

Options:
{options}

{after-help}
"
)]
pub struct Args {
    #[arg(
        help = "Output object file",
        long_help = "The path where the final linked object file (.o) will be written.",
        short
    )]
    output: PathBuf,

    #[arg(
        long,
        default_value = "mwccpsp.exe",
        help = "Path to the Metrowerks C Compiler executable.",
        long_help = "The path to the legacy Metrowerks C Compiler (e.g., mwccpsp.exe or mwccmips.exe). If running on a non-Windows host, this will be executed via Wine or wibo depending on the --use-wibo flag."
    )]
    mwcc_path: PathBuf,

    #[arg(
        long,
        default_value = "mipsel-linux-gnu-as",
        help = "Path to the GNU assembler (usually mipsel-linux-gnu-as).",
        long_help = "The assembler used to compile the raw .s files referenced by INCLUDE_ASM. This is typically a modern GNU assembler provided by your system's package manager."
    )]
    as_path: String,

    #[arg(
        long,
        default_value = "allegrex",
        help = "Target architecture flag passed to the assembler.",
        long_help = "Standard GNU binutils arguments (e.g. -march=allegrex for PSP/MIPS targeted compilation)."
    )]
    as_march: String,

    #[arg(
        long,
        default_value = "32",
        help = "MABI flag passed to the assembler.",
        long_help = "Standard GNU binutils arguments (e.g. -mabi=32 for PSP/MIPS targeted compilation)."
    )]
    as_mabi: String,

    #[arg(
        long,
        help = "Use 'wibo' to execute Windows binaries instead of Wine.",
        long_help = "When enabled, metrowrap will invoke the MWCC compiler using the lightweight 'wibo' wrapper instead of standard Wine. Wibo is significantly faster for pure CLI tools but requires wibo to be installed and available in the system PATH."
    )]
    use_wibo: bool,

    #[arg(
        long,
        default_value = "wibo",
        help = "Path to the 'wibo' executable if --use-wibo is enabled."
    )]
    wibo_path: PathBuf,

    #[arg(
        long,
        help = "Directory prefix utilized for locating INCLUDE_ASM relative includes.",
        long_help = "If your INCLUDE_ASM macros reference paths relative to a specific base directory (such as 'asm/nonmatchings/'), provide that prefix here so metrowrap can locate the corresponding .s files."
    )]
    asm_dir: Option<PathBuf>,

    #[arg(
        long,
        help = "Path to a GNU assembly macro includes file (.inc).",
        long_help = "Passed to the assembler as an include file. Useful for providing standardized instruction macros to all extracted assembly blocks during decompilation."
    )]
    macro_inc_path: Option<PathBuf>,

    #[arg(long, hide = true)]
    src_dir: Option<PathBuf>,

    #[arg(long, hide = true)]
    target_encoding: Option<String>,

    /// compile INCLUDE_ASM funcs as NOP stubs, useful for progress reporting
    #[arg(long)]
    skip_asm: bool,

    /// Keep temp files on failure for debugging.
    ///
    /// Without a value: writes files to the system temp dir and leaves them
    /// in place when the build fails so you can inspect them.
    ///
    /// With `=shm`: uses /dev/shm if available (falling back to system temp).
    /// Files are always cleaned up at process exit - nothing is left orphaned
    /// in /dev/shm - but they survive long enough for you to inspect them
    /// before the process terminates.
    #[arg(
        long,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "tmp",
        value_name = "shm"
    )]
    debug_keep_temp_files_on_failure: Option<String>,

    /// This catches everything else: unknown flags AND the file path.
    /// trailing_var_arg means everything after the first "unknown"
    /// or positional is dumped here.
    #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
    rest: Vec<String>,
}

fn main() {
    let args = Args::parse();
    if let Err(e) = run(args) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

pub fn run(mut args: Args) -> Result<(), Box<dyn Error>> {
    if let Some(encoding) = args.target_encoding {
        return Err(format!("--target-encoding is no longer supported, use `iconv --from-code=UTF-8 --to-code={encoding}` instead").into());
    }

    let Some(possible_infile) = args.rest.last() else {
        return Err("missing input file".into());
    };

    let infile = if possible_infile == "-" || PathBuf::from(possible_infile).is_file() {
        possible_infile.clone()
    } else {
        return Err(format!("cannot find input file: {possible_infile}").into());
    };

    args.rest.pop();

    let temp_mode = match args.debug_keep_temp_files_on_failure.as_deref() {
        None => TempMode::Normal,
        Some("shm") => TempMode::ShmDebug,
        Some(_) => TempMode::KeepOnFailure,
    };

    let workspace = Workspace::new(temp_mode)?;

    if args.src_dir.is_some() {
        eprintln!(
            "warning: --src-dir is deprecated and will be removed in a future version. use -I instead."
        );
    }

    let compiler =
        compiler::Compiler::new(args.rest, args.mwcc_path, args.use_wibo, args.wibo_path);

    let assembler = assembler::Assembler {
        as_path: args.as_path,
        as_march: args.as_march,
        as_mabi: args.as_mabi,
        as_flags: vec!["-G0".to_string()],
        macro_inc_path: args.macro_inc_path,
    };

    let preprocessor = preprocessor::Preprocessor::new(args.asm_dir);

    let c_reader = read_source(&infile, args.src_dir.as_deref())?;

    if let Err(e) = metrowrap::process_c_file(
        &c_reader,
        &args.output,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace,
        args.skip_asm,
    ) {
        workspace.on_failure();
        return Err(format!("failed to process c file: {:?}", e).into());
    }

    Ok(())
}

fn read_source(
    infile: &str,
    src_dir_arg: Option<&std::path::Path>,
) -> Result<NamedSource, Box<dyn Error>> {
    if infile == "-" {
        let mut content = Vec::new();
        io::stdin().lock().read_to_end(&mut content)?;
        Ok(NamedSource {
            source: SourceType::StdIn,
            content,
            src_dir: src_dir_arg
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf(),
        })
    } else {
        let mut content = Vec::new();
        File::open(infile)?.read_to_end(&mut content)?;
        Ok(NamedSource {
            source: SourceType::Path(infile.to_string()),
            content,
            src_dir: PathBuf::from(infile)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[test]
    fn test_short_help() {
        let mut cmd = Args::command();
        let help = cmd.render_help().to_string();

        // Assert that the short help contains the basic description
        assert!(help.contains("An advanced Metrowerks compiler wrapper"));

        // It should contain short help args
        assert!(help.contains("Output object file"));
        assert!(help.contains("Target architecture flag"));

        // It should NOT contain the incredibly verbose long descriptions
        assert!(!help.contains("mwccgap"));
        assert!(!help.contains("EXAMPLES:"));
    }

    #[test]
    fn test_long_help() {
        let mut cmd = Args::command();
        let help = cmd.render_long_help().to_string();

        // Assert that the long help contains the exhaustive descriptions
        assert!(help.contains("metrowrap is a modern, high-performance drop-in replacement"));
        assert!(help.contains("mwccgap"));

        // Verify the extra sections appended at the end
        assert!(help.contains("EXAMPLES:"));
        assert!(help.contains("EXIT STATUS:"));
        assert!(help.contains("BUG REPORTING:"));

        // Verify a specific long_help from an argument
        assert!(help.contains("Standard GNU binutils arguments"));
    }

    #[test]
    fn test_rejects_target_encoding() {
        let args = Args::parse_from(&[
            "mw",
            "--target-encoding",
            "Shift-JIS",
            "-o",
            "out.o",
            "file.c",
        ]);
        let result = run(args);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("--target-encoding is no longer supported"));
    }

    #[test]
    fn test_missing_input_file() {
        let args = Args::parse_from(&["mw", "-o", "out.o"]);
        let result = run(args);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("missing input file"));
    }

    #[test]
    fn test_nonexistent_input_file() {
        let args = Args::parse_from(&["mw", "-o", "out.o", "does_not_exist.c"]);
        let result = run(args);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("cannot find input file"));
    }
}
