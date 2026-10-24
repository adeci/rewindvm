# Fuzz targets

Each target feeds arbitrary bytes to one reader of what a run writes or
what a guest answers, the readers that see files from other machines
(an imported `.rwd`, a trace the browser viewer downloads) and answers
from inside the VM:

| Target      | Reader                                                                                 |
| ----------- | -------------------------------------------------------------------------------------- |
| `trace`     | a trace file: its records, events, and read and write records                          |
| `record`    | one guest record, as an event and as read and write records                            |
| `keyframe`  | a keyframe file                                                                        |
| `manifest`  | a run's manifest                                                                       |
| `maps`      | the guest's answer to a `running` inspection                                           |
| `elf`       | the ELF readers gdb's helpers use: load offset, dynamic section, gdb scripts, build ID |
| `file_diff` | two versions of a file, compared as the app shows a change                             |

The directory is a Cargo workspace of its own, so libFuzzer and the
instrumented builds stay out of the engine's builds.

## Running one

```
nix develop .#fuzz
cd fuzz
cargo fuzz run -s none trace corpus/trace ../crates/rewind-core/tests/formats
```

The first directory is where libFuzzer keeps the inputs it finds; the
ones after it are seeds, here the format samples the engine's tests
check in. The `elf` target does better seeded with a real ELF file
copied into `corpus/elf`.

`-s none` builds without AddressSanitizer: nixpkgs' rustc does not ship
its runtime. The crates fuzzed here forbid unsafe code, so what a fuzzer
finds in them is a panic, which libFuzzer catches without a sanitizer.

## When one crashes

`cargo fuzz run` prints the input it crashed on and saves it under
`artifacts/`. Fix the reader in the crate the panic is in, with a unit
test there that builds the input by hand and fails without the fix. The
unit test is what keeps the bug fixed: the package build runs it, and
nothing builds or runs this directory.
