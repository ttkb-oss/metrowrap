// SPDX-FileCopyrightText: © 2026 TTKB, LLC
// SPDX-License-Identifier: BSD-3-CLAUSE
use std::error::Error;
use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;

use metrowrap::NamedSource;
use metrowrap::SourceType;
use metrowrap::assembler;
use metrowrap::compiler;
use metrowrap::preprocessor;
use metrowrap::workspace::{TempMode, Workspace};

#[derive(Parser, Debug)]
#[command(author, version, about = "MWCC bridge for assembly injection")]
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
"
)]
struct Args {
    #[arg(help = "Output object file", short)]
    output: PathBuf,

    #[arg(long, default_value = "mwccpsp.exe")]
    mwcc_path: PathBuf,

    #[arg(long, default_value = "mipsel-linux-gnu-as")]
    as_path: String,

    #[arg(long, default_value = "allegrex")]
    as_march: String,

    #[arg(long, default_value = "32")]
    as_mabi: String,

    #[arg(long)]
    use_wibo: bool,

    #[arg(long, default_value = "wibo")]
    wibo_path: PathBuf,

    #[arg(long)]
    asm_dir: Option<PathBuf>,

    #[arg(long)]
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

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = Args::parse();

    if let Some(encoding) = args.target_encoding {
        eprintln!(
            "--target-encoding is no longer supported, use `iconv --from-code=UTF-8 --to-code={encoding}` instead"
        );
        std::process::exit(1);
    }

    let Some(possible_infile) = args.rest.last() else {
        eprintln!("missing input file");
        std::process::exit(1);
    };

    let infile = if possible_infile == "-" || PathBuf::from(possible_infile).is_file() {
        possible_infile.clone()
    } else {
        eprintln!("cannot find input file: {possible_infile}");
        std::process::exit(1);
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

    let preprocessor = Arc::new(preprocessor::Preprocessor {
        asm_dir_prefix: args.asm_dir,
    });

    let c_reader = if infile == "-" {
        let mut content = Vec::new();
        io::stdin().lock().read_to_end(&mut content)?;
        NamedSource {
            source: SourceType::StdIn,
            content,
            src_dir: args.src_dir.unwrap_or(PathBuf::from(".")),
        }
    } else {
        let mut content = Vec::new();
        File::open(&infile)?.read_to_end(&mut content)?;
        NamedSource {
            source: SourceType::Path(infile.clone()),
            content,
            src_dir: PathBuf::from(infile)
                .parent()
                .unwrap_or(&PathBuf::from("."))
                .to_path_buf(),
        }
    };

    if let Err(e) = metrowrap::process_c_file(
        &c_reader,
        &args.output,
        &preprocessor,
        &compiler,
        &assembler,
        &workspace,
        args.skip_asm,
    ) {
        eprintln!("failed to process c file: {:?}", e);
        workspace.on_failure();
        std::process::exit(1);
    }

    Ok(())
}
