To Do
=====


[] support preprocessor output
[] modify args to gcc style
[] base `as_flags` arg on sdatathreshold (as comented in mwccgap.py)
[] integrate pspas
[] look into more standardized args for asm-dir/macro-inc-path



support GCC options for assembler:
  Passing Options to the Assembler
       You can pass options to the assembler.

       -Wa,option
           Pass option as an option to the assembler.  If option contains commas, it is split into multiple options at the commas.

       -Xassembler option
           Pass option as an option to the assembler.  You can use this to supply system-specific assembler options that GCC does not recognize.

           If you want to pass an option that takes an argument, you must use -Xassembler twice, once for the option and once for the argument.

To Done
=======

[x] see if all communication with mwcc can be through stdin/stdout/stderr (answer: doesn't look like it)
[x] support stdin
[x] parallelize s file compilation
[x] use rayon to avoid clone when parallel processing `find_macros`
[x] translate error paths
[x] compare each stage to mwccgap and snapshot commit at that point
  [x] then modify args to GCC style
[x] avoid reading ASM files at all
[x] use /dev/shm instead of a temp file for assembler output
[x] use a single tmpdir workspace
[x] currently, stdin is written next to the original file using the `--src-dir` option. that should be more flexible.
   instead of --src-dir, -I can be used to fix search paths
[x] see if s files can be inlined (they cannot)
