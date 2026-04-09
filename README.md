# <div width="128"><center><img align="center" src="docs/MetroWrap@0.5x.png" alt="MetroWrap" height="128"><br/>MetroWrap</center></div>

[![CI](https://github.com/ttkb-oss/metrowrap/actions/workflows/ci.yml/badge.svg)](https://github.com/ttkb-oss/metrowrap/actions/workflows/ci.yml)
[![Coverage](https://codecov.io/gh/ttkb-oss/metrowrap/branch/main/graph/badge.svg)](https://codecov.io/gh/ttkb-oss/metrowrap)
[![Crates.io](https://img.shields.io/crates/v/metrowrap.svg)](https://crates.io/crates/metrowrap)
[![License: BSD-3-Clause](https://img.shields.io/badge/license-BSD--3--Clause-blue.svg)](LICENSE)

MetroWrap is a wrapper for the Metrowerks CodeWarrior C compiler (`mwcc`) that enables
assembly injection into compiled objects. It is inspired by and compatible with the
workflow established by [`mwccgap`](https://github.com/mkst/mwccgap), with a focus on
performance and build system integration.

---

## Background

Decompilation projects targeting platforms like the PSP might use Metrowerks' CodeWarrior
compilers (`mwcc`) to match original build output. These projects frequently mix C
source with disassembled code segments and data via `INCLUDE_ASM` and `INCLUDE_RODATA`
macros to "inline" assembly, which `mwcc` cannot handle on its own.

[`mwccgap`](https://github.com/mkst/mwccgap) solved this elegantly: support GCC-style
inline assembly by preprocessing the C file to find `INCLUDE_ASM` directives, compile
stub replacements, assemble the `.s` files, and splice the assembled sections into the
compiled object. MetroWrap builds on the same idea but with less overhead and additional
compiler feature support.

## Why MetroWrap?

MetroWrap improves on `mwccgap` in several ways, especially when working on large projects:

**Dependency file support.** Without accurate `.d` files, build systems like Make and
Ninja cannot determine which object files need recompilation when a header changes.
On a project with thousands of C files this means either always rebuilding everything
or silently producing stale objects. Dependency support was proposed for `mwccgap` but
stalled in review, which was the immediate motivation to start MetroWrap.

**Speed.** A Rust implementation running the same algorithm is roughly twice as fast as
the Python equivalent. Around 80% of that improvement comes from the language itself;
another 10-20% comes from additional optimizations when producing output that combines
C and assembly files. Half as many calls to `mwcc` are required when `INCLUDE_ASM` or
`INCLUDE_RODATA` macros are present and assembly files are assembled in parallel. For
projects with hundreds or thousands of files this adds up.

On `sotn-decomp`, `mwccgap` took around 19s to build all PSP files (approximately 25%
of the game). MetroWrap does the same around 10s.

**Better diagnostics.** `mwcc` is a command-line windows binary that emits Windows-style
paths and refers to temp files by their generated names. MetroWrap filters its output to
show POSIX paths pointing back to the original source files, so warnings and errors land
where your editor expects them.

**Encoding agnostic.** `metrowrap` supports C source files in any encoding that is an
ASCII superset. It doesn't concern itself with the assembly format. It works with
[`spim`](https://github.com/Decompollaborate/spimdisasm.git), but isn't tied to its
conventions or macros.

**Tempfile handling.** Temp files are consolidated in a single workspace and will use
shared memory if available.

## How It Works

Given a C file that uses `INCLUDE_ASM`:

```c
// src/player.c
#include "common.h"

INCLUDE_ASM("asm/player", Player_Update);

s32 Player_GetHealth(Player *p) {
    return p->health;
}
```

MetroWrap:

1. Scans the source for `INCLUDE_ASM` and `INCLUDE_RODATA` macros.
2. If none are found, compiles the original file directly - no overhead.
3. If macros are present, generates a stub file with nop bodies and compiles that once.
4. Assembles each referenced `.s` file.
5. Splices the assembled `.text` and `.rodata` sections into the compiled object in place
   of the stubs, adjusting symbol tables and relocation records accordingly.
6. Writes the final `.o` file and, if requested, a dependency file.

The result is an object file identical to what a build system that handles assembly
injection natively would produce.

## Installation

### From crates.io

```sh
cargo install metrowrap
```

### From source

```sh
git clone https://github.com/ttkb-oss/metrowrap
cd metrowrap
cargo build --release
# Binary is at target/release/mw
```

Requires Rust 1.87 or later.

## Usage

```
mw [OPTIONS]… -o <output> [COMPILER_FLAGS]… <input>
```

The input file may be `-` to read from stdin, which is useful when another tool
preprocesses the source before compilation.

### MetroWrap options

| Flag | Default | Description |
|------|---------|-------------|
| `-o <path>` | *(required)* | Output object file |
| `--mwcc-path <path>` | `mwccpsp.exe` | Path to the MWCC compiler binary |
| `--use-wibo` | off | Run MWCC under [wibo](https://github.com/decompals/wibo) |
| `--wibo-path <path>` | `wibo` | Path to the wibo binary |
| `--as-path <path>` | `mipsel-linux-gnu-as` | Path to the assembler |
| `--as-march <arch>` | `allegrex` | Assembler `-march` value |
| `--as-mabi <abi>` | `32` | Assembler `-mabi` value |
| `--asm-dir <path>` | *(none)* | Root directory to resolve `INCLUDE_ASM` paths against |
| `--macro-inc-path <path>` | *(none)* | Assembly macro include file prepended to each `.s` file |

All other flags (anything starting with `-` that MetroWrap does not recognise, plus
everything before the input file) are forwarded directly to MWCC.

### Dependency files

MetroWrap supports MWCC's dependency file flags. Pass `-gccdep -MD` (or `-MMD` to
exclude system headers) in the compiler flags alongside a regular GCC-style build rule
to get accurate incremental rebuilds:

- With `-gccdep`: the `.d` file is written next to the output `.o` as `<name>.o.d`.
- Without `-gccdep` (MWCC's native mode): the `.d` file is written next to the source
  as `<name>.d`.

### Example — PSP decomp project

```sh
mw \
  -o build/src/player.o \
  --mwcc-path bin/mwccpsp.exe \
  --use-wibo --wibo-path bin/wibo \
  --as-path tools/allegrex-as \
  --asm-dir asm/pspeu \
  --macro-inc-path include/macro.inc \
  -gccinc -gccdep -MD \
  -Iinclude -Iinclude/pspsdk \
  -D_internal_version_pspeu \
  -c -lang c -sdatathreshold 0 -char unsigned \
  -fl divbyzerocheck -Op -opt nointrinsics \
  src/player.c
```

### Example — reading from stdin

Some toolchains preprocess source through a separate tool before compilation.
MetroWrap accepts `-` as the input file:

```sh
sotn_str process -p -f src/dialogue.c | \
  iconv --from-code=UTF-8 --to-code=Shift-JIS | \
  mw \
    -o build/src/dialogue.o \
    --mwcc-path bin/mwccpsp.exe \
    --use-wibo \
    --asm-dir asm/pspeu \
    [compiler flags…] \
    -
```

### Diagnostic output

MWCC emits diagnostics to stdout using Windows-style paths and the names of
whatever temp files were passed to it. MetroWrap rewrites these in place so the
output is useful:

```
# Before:
#      In: src\st\e_grave_keeper.h
#    From: src\st\are\.tmpJ8qy3h.c

# After:
#      In: src/st/e_grave_keeper.h
#    From: src/st/are/e_grave_keeper.c
```

## Build system integration

### Ninja (via the Python interface)

The example below is drawn from a PSP decompilation project using
[`ninja_syntax`](https://pypi.org/project/ninja-syntax/). It demonstrates several
MetroWrap features working together:

- Source is fed through **stdin** (`-`) after being preprocessed by `sotn_str`, so
  Ninja's `$in` variable names the original `.c` file while the compiler never sees it
  directly.
- A **custom assembler** (`pspas`) is used in place of the default `mipsel-linux-gnu-as`.
- The source is **re-encoded to Shift-JIS** using `iconv(1)` before being passed to
  MetroWrap, which expects that encoding for certain projects.
- **GCC-style dependency files** are generated with `-gccdep -MD` and wired into Ninja
  via `depfile`/`deps`, so Ninja automatically tracks header changes and invalidates
  only the objects that need rebuilding.

```python
nw.rule(
    "psp-cc",
    command=(
        "VERSION=$version"
        " tools/sotn_str/target/release/sotn_str process -p -f $in"
        " | iconv --from-code=UTF-8 --to-code=$encoding"
        " | mw"
        " -o $out"
        " --mwcc-path bin/mwccpsp.exe"
        " --use-wibo --wibo-path bin/wibo"
        " --as-path tools/pspas/target/release/pspas"
        " --asm-dir asm/pspeu"
        " --macro-inc-path include/macro.inc"
        " -gccdep"
        " -MD"          # -MD includes angle-bracket headers; use -MMD to exclude them
        " -gccinc"
        " -I$src_dir"
        " -Iinclude -Iinclude/pspsdk"
        f" -D_internal_version_$version -DSOTN_STR {extra_cpp_defs}"
        " -c -lang c -sdatathreshold 0 -char unsigned -fl divbyzerocheck"
        " $opt_level -opt nointrinsics"
        " -"            # read preprocessed source from stdin
    ),
    description="psp cc $in",
    depfile="$out.d",   # MetroWrap writes <output>.o.d when -gccdep is active
    deps="gcc",         # Ninja reads and internalises the depfile after each build
)
```

The `depfile="$out.d"` / `deps="gcc"` pair is the key to fast incremental builds.
After each successful compilation Ninja reads the generated `.d` file, records the
header dependencies in its own database, and deletes the `.d` file. On subsequent runs
it rebuilds an object only when the source file or any of its recorded headers have
changed.

This modifies `mwcc`'s dependency behavior by changing the output file to the object
output with `.d` appended. This matches GCC and Clang's behavior. `mwcc` would normally
replace the `.o` with a `.d` extension which makes Ninja rule construction more difficult.

## Migrating from mwccgap

MetroWrap is designed to be a drop-in replacement in most build systems. The main
interface difference is that MetroWrap follows the GCC convention of `-o <output>`
as a flag rather than a positional argument, and all compiler flags are passed
through transparently.

| mwccgap | MetroWrap |
|---------|-----------|
| `mwccgap.py src/foo.c foo.o --mwcc-path …` | `mw -o foo.o … src/foo.c` |
| `--mwcc-path` | `--mwcc-path` |
| `--use-wibo` / `--wibo-path` | `--use-wibo` / `--wibo-path` |
| `--as-path` | `--as-path` |
| `--asm-dir-prefix` | `--asm-dir` |
| `--macro-inc-path` | `--macro-inc-path` |
| `--src-dir` | Use the `-I` compiler option |
| `--target-encoding` | Use `iconv(1)` or similar |
| *(not supported)* | `-gccdep -MD` / `-MMD` |

`mwccgap`'s `--target-encoding` option is no longer supported and should be done by `iconv`
or the encoding of the source file. Any ASCII superset should work with MetroWrap.

`mwccgap`'s `--src-dir` is used when compiling source from stdin. MetroWrap uses the
standard `-I` compiler option instead.

# TODO

* Args were ported directly from `mwccgap` to reduce replacement complexity, but
  don't necessarily follow the conventions used by `gcc` or `clang`. These may change
  in the future to use `-Wa,…`, `-fuse-ld…`, and other style args.
* `sotn-decomp` uses a custom `pspas` to convert assembly to a format recognizable
  by `allegrex-as`. Some investigation should be done to determine if that should just
  be built in.

# Users

[Let me know if you're using Metrowrap!](https://github.com/ttkb-oss/metrowrap/issues/new?title=I%27m%20Using%20Metrowrap%20for%20%3Cproject%3E&body=Project%20URL%3A%20%3Curl%3E%0AProject%20repo%3A%20%3Curl%3E%0A%3Cgeneral%20project%20description%3E&labels=user)

## Projects

* [Castlevania: Symphony of the Night Decompilation](https://github.com/Xeeynamo/sotn-decomp)

## License

BSD-3-Clause. See [LICENSE](LICENSE).

MetroWrap is not affiliated with, endorsed by, or derived from the `mwccgap` project
or its authors. The Metrowerks and CodeWarrior names are trademarks of their respective
owners.
