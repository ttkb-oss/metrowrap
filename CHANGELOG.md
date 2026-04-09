0.2.0
=====

n.b.! This is the first behavioral departure from `mwccgap`. Re-encoding source files is no longer supported and
must be performed by another tool (e.g. iconv) or the source can be saved in the desired encoding.

### Removed

* The `--target-encoding` option has been removed. When passing in sources via stdin use iconv for conversion. For
  direct file compilation the source encoding will be used. Encodings must be ASCII supersets (e.g. UTF-8, Shift-JIS).

### Deprecated

* The `--src-dir` option is deprecated. Use `-I` instead.

### Changed

* `mw` is now agnostic regarding the ASM format being used and knows nothing about their content. Only the assembler
  needs to understand them. ASM imported with `INCLUDE_ASM` must still contain exactly one `.text` symbol and zero or
  more `.rodata` symbols. When included with `INCLUDE_RODATA` there must be exactly one `.rodata` symbol and no other
  symbols.
* All temp files are now written to a single shared `tmpdir` workspace
* When compiling from `stdin` the input source is no longer written to `--src-dir`
* If `/dev/shm` is available, it will be used for temporary files to reduce disk IO
* Macro scanning (`INCLUDE_ASM`/`INCLUDE_RODATA`) is faster and operates directly on raw bytes
* Internal data copying has been reduced
* Various Performance optimizations

### Added

* Added a `--debug-keep-temp-files-on-failure` option to see any temp files created when there is any failure

### Fixed

* Fixes bug if the `--macro-inc` parent dir path is empty

0.1.3
=====

### Added

* Support "endlabel" and "nonmatching" lines (@ThirstyWraith)
* Add `curl` to debian dependencies

0.1.2
=====

### Added

* Support `File:` diagnostics

### Fixed

* Fix conditional filter formatting

0.1.1
=====

### Added

* Add support for conditional `INCLUDE` macros

0.1.0
=====

* Initial release
