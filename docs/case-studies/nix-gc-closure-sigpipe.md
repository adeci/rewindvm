# Case study: a SIGPIPE in Nix's gc-closure test

Nix's functional test `gc-closure.sh` can fail with exit status 141. Rewind VM
failed it on the first run; the host passed it 200 times. We reported it as
[NixOS/nix#16546](https://github.com/NixOS/nix/issues/16546), and the fix
below was merged as [NixOS/nix#16547](https://github.com/NixOS/nix/pull/16547).

Scrub the recording in your browser: [open it in the viewer](https://rewindvm.dev/viewer/?url=examples/nix-gc-closure.rwd).

## The test

Lines 14 to 16 of
[gc-closure.sh](https://github.com/NixOS/nix/blob/203f85b2e851fc52e253e8e33eff5fb92936736a/tests/functional/gc-closure.sh#L14-L16)
split a derivation's two outputs:

```bash
    input2=$(nix build -f dependencies2.nix input2_drv --no-link --print-out-paths)
    input2_out=$(printf "%s" "$input2" | head -n1)
    input2_out2=$(printf "%s" "$input2" | tail -n1)
```

Meson runs each functional test as `bash -x -e -u -o pipefail`, so if `printf`
dies the pipeline fails and the test ends.

## Run it

The derivation is the functional tests of
[Nix master 203f85b2](https://github.com/NixOS/nix/commit/203f85b2e851fc52e253e8e33eff5fb92936736a),
narrowed to this one test
([flake](../../examples/case-studies/flake.nix)). `--epoch` fixes the VM's
clock, which seeds its randomness, so these are the runs you get. It fails on
schedule 0:

```console
$ rewind nix --epoch 1791936000 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-git-gc-closure'
rewind: packing 217 store paths for nix-functional-gc-closure-2.36pre20260912_203f85b2
...
+(gc-closure.sh:45) nix_gc_closure false
...
++(gc-closure.sh:14) nix build -f dependencies2.nix input2_drv --no-link --print-out-paths
...
+(gc-closure.sh:14) input2=$'/build/nix-test/main/gc-closure/store/ij8qwx0m6dvpmggqmq8zmhbaiw6ym8pg-dependencies-input-2\n/build/nix-test/main/gc-closure/store/hw6dm6rpyn ...
++(gc-closure.sh:15) printf %s $'/build/nix-test/main/gc-closure/store/ij8qwx0m6dvpmggqmq8zmhbaiw6ym8pg-dependencies-input-2\n/build/nix-test/main/gc-closure/store/hw6dm6 ...
++(gc-closure.sh:15) head -n1
+(gc-closure.sh:15) input2_out=/build/nix-test/main/gc-closure/store/ij8qwx0m6dvpmggqmq8zmhbaiw6ym8pg-dependencies-input-2
...
1/1 main - nix-functional-tests:gc-closure FAIL             0.26s   (exit status 141 or signal 13 SIGPIPE)
...
rewind: run e413b6dfe9a4598f exited:1 after 37496 steps, 1.436s virtual, 7.781s wall (poweroff)
```

```console
$ rewind replay e413b6df
identical to the recording: 9801 events and 16923 read and write records over 37496 steps, and the whole machine at its 23 keyframes
```

`input2_out` got the right value, and the test still died of signal 13.

## What died

The events around the failure, and the processes just before it:

```console
$ rewind events e413b6df --from 37229 --to 37329
     37239   359/359   fork() = 416
     37242   416/416   fork() = 417
     37245   416/416   fork() = 418
     37275   418/418   execve("/nix/store/2gfxiwls9hbgwdwcy43mprchwsq36mg6-coreutils-9.11/bin/head", ["head", "-n1"])
     37298   418/418   exit_group(head) exited:0
     37304   417/417   SIGPIPE code=0 addr=0x0
     37305   417/417   exit_group(bash) killed:SIGPIPE
     37307   416/416   SIGCHLD code=1 addr=0x0
     37308   416/416   exit_group(bash) exited:141
     37311   359/359   SIGCHLD code=1 addr=0x0
     37324   359/359   exit_group(bash) exited:141
     37328   358/358   close("anon_inode:[pidfd]")

$ rewind ps e413b6df 37281
     1 /init
    34   bash -e /nix/store/l622p70vy8k5sh7y5wizi5f2mic6ynpg-source-stdenv.sh /nix/store/shkw4qm9qcw5sc5n1k5jznc83ny02r39-default-builder.sh
   358     /nix/store/lb41b0anx1f98y9y5s9mdv97gjgsq740-python3-3.14.7/bin/python3.14 /nix/store/r72868g8znkk4m8pmwsn3cf8v3h84mr0-meson-1.12.1/bin/meson test --no-rebuild --print-errorlogs gc-closure
   359       /nix/store/10dxp0qxqxxsyiljrh2kp0xqhz6arhcx-bash-5.3p15/bin/bash -x -e -u -o pipefail gc-closure.sh
   416         /nix/store/10dxp0qxqxxsyiljrh2kp0xqhz6arhcx-bash-5.3p15/bin/bash -x -e -u -o pipefail gc-closure.sh (fork)
   417           /nix/store/10dxp0qxqxxsyiljrh2kp0xqhz6arhcx-bash-5.3p15/bin/bash -x -e -u -o pipefail gc-closure.sh (fork)
   418           head -n1
```

416 is the command substitution, 417 runs `printf` and 418
runs `head`. `head` exits 0, then 417 dies of SIGPIPE and the test
exits 141.

## Count the writes

`rewind cat` reads a file in a fork of the run at any step. `/proc/<pid>/io`
counts each process's writes:

```console
$ for step in 37294 37298 37304; do echo "step $step"; rewind cat e413b6df $step /proc/417/io | grep -E '^(wchar|syscw)' | sed 's/^/  417 /'; rewind cat e413b6df $step /proc/418/io | grep -E '^(wchar|syscw)' | sed 's/^/  418 /'; done
step 37294
  417 wchar: 224
  417 syscw: 1
  418 wchar: 30
  418 syscw: 1
step 37298
  417 wchar: 316
  417 syscw: 2
  418 wchar: 122
  418 syscw: 2
step 37304
  417 wchar: 316
  417 syscw: 3
rewind: /proc/418/io: no such file at this step
```

The first write of each is its `bash -x` trace line. By 37298 `printf`
has written the first store path (92 bytes) and `head` has echoed
it and exited. At 37304 `printf`'s third write adds no bytes: the pipe
has no reader left.

## Root cause

bash line-buffers its own standard output, so `printf` writes one line per
`write` call. On the host, with the same bash and the two paths from the run:

```console
$ export X=$'/build/nix-test/main/gc-closure/store/ij8qwx0m6dvpmggqmq8zmhbaiw6ym8pg-dependencies-input-2\n/build/nix-test/main/gc-closure/store/hw6dm6rpynd7hmhl1mnp0z4m28lfbx4b-dependencies-input-2-out2'
$ strace -f -e trace=write -o /dev/stdout bash -c 'set -o pipefail; y=$(printf "%s" "$X" | cat)' | grep write
1281845 write(1, "/build/nix-test/main/gc-closure/"..., 92) = 92
1281845 write(1, "/build/nix-test/main/gc-closure/"..., 96) = 96
```

The buffering is set in `shell_initialize` in bash 5.3's
[shell.c](https://git.savannah.gnu.org/cgit/bash.git/tree/shell.c?h=bash-5.3).
`head -n1` exits after the first line. If it does so between the two writes,
the second write raises SIGPIPE, `pipefail` makes the pipeline's status 141,
and `-e` ends the test.

## Who had the CPU

`rewind threads` prints which process held the CPU through each stretch of
steps:

```console
$ rewind threads e413b6df --from 37273 --to 37309
     37273      37273       358/358  meson
     37274      37274       418/418  bash
     37275      37290       418/418  head
     37291      37295       417/417  bash
     37296      37296     kernel 11  ksoftirqd/0
     37297      37298       418/418  head
     37299      37299     kernel 11  ksoftirqd/0
     37300      37301       359/359  bash
     37302      37302       416/416  bash
     37303      37303     kernel 11  ksoftirqd/0
     37304      37305       417/417  bash
     37306      37306     kernel 11  ksoftirqd/0
     37307      37308       416/416  bash
     37309      37309     kernel 11  ksoftirqd/0
```

`head` blocks in `read` by 37290. `printf` makes its first write at
37291, which wakes `head`. `head` runs at 37297 and exits,
and `printf` only gets the CPU back at 37304.

## How often it fails

```console
$ rewind check --epoch 1791936000 --all --schedules 256 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-git-gc-closure'
schedule   0: exited:1            37496 steps    run e413b6dfe9a4598f
schedule   1: exited:0            94286 steps  a50a5ab6d992  run 96822979453da6c7
...
schedule  64: exited:1            84696 steps    run 47ec4b096fc1926a
...
schedule 0 failed; 255 of 256 other schedules ended differently

schedule 1 passes where schedule 0 fails; finding the step that decides it
schedule 1 limited to steps 37291..44429 still ends differently
step 44428 decides it: a request to switch threads and a 80 µs pause of the running thread there makes the run pass

passing: run 5ecc968f48287a0f, schedule 1 over steps 37291..44429
failing: run 69e21f691f4cb828, schedule 1 over steps 37291..44428
the two are the same run until step 44428

where /nix/store/10dxp0qxqxxsyiljrh2kp0xqhz6arhcx-bash-5.3p15/bin/bash -x -e -u -o pipefail gc-closure.sh first behaves differently:
  both          75095   359/359   fork() = 534
  both          75098   534/534   fork() = 535
  both          75101   534/534   fork() = 536
  failing       75160   535/535   SIGPIPE code=0 addr=0x0
  failing       75161   535/535   exit_group(bash) killed:SIGPIPE
  failing       75163   534/534   SIGCHLD code=1 addr=0x0
  failing       75164   534/534   exit_group(bash) exited:141
  passing       75203   535/535   exit_group(bash) exited:0
  passing       75242   534/534   SIGCHLD code=1 addr=0x0
  passing       75243   534/534   exit_group(bash) exited:0
  passing       75245   359/359   SIGCHLD code=1 addr=0x0

open both in the desktop app: rewind open 69e21f691f4cb828 75160 --compare 5ecc968f48287a0f
```

Schedules 0 and 64 fail; the other 255 pass. Here the unperturbed
schedule happens to be the failing order. The test runs the pipeline three
times, so the narrowed pair parts at a later call, step 75160.

```console
$ rewind check --run e413b6df --schedule-from 37242 --schedules 64 --all --no-narrow
schedule   0: exited:1            37496 steps    run e413b6dfe9a4598f
schedule   1: exited:0            88450 steps  a50a5ab6d992  run cb63a60e00db969f
schedule   2: exited:0            88506 steps  a50a5ab6d992  run 129aa7359e747794
...
schedule  18: exited:1            78278 steps    run a8fe93d5f7446334
...
schedule  64: exited:0            88363 steps  a50a5ab6d992  run 414a4e13e3b0f3a3
run e413b6dfe9a4598f failed; 63 of 64 other schedules ended differently
```

Perturbing only from step 37242, where the pipeline forks `printf`,
moves 63 of 64 schedules off the failing order. Schedule
18 fails the same way in a later call of the pipeline: `head`
exits between `printf`'s two writes, and `printf`, process 535,
dies of SIGPIPE.

```console
$ rewind check --epoch 1791936000 --all --schedules 1024 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-gc-closure'
rewind: packing 217 store paths for nix-functional-gc-closure-2.35.2
schedule   0: exited:1            36762 steps    run dabeaa8b5b119d40
...
schedule 539: exited:1            63371 steps    run b1f13d894578d7e7
...
schedule 777: exited:1            42688 steps    run 2c13f9487b58d253
...
schedule 0 failed; 1022 of 1024 other schedules ended differently
...
```

Nix 2.35.2 from nixpkgs has the same line. It fails schedule 0 too, and
3 of its 1025 runs fail, all with SIGPIPE.

On the host it fails only when the writer is made the low-priority task on its
CPU:

| What ran on the host                                       | Failed       |
| ---------------------------------------------------------- | ------------ |
| `gc-closure` from Nix master, 200 times in one build       | 0 of 200     |
| line 15 in a loop, 16 CPUs                                 | 0 of 5000    |
| line 15 in a loop, pinned to one CPU                       | 0 of 5000    |
| the same, with a busy loop sharing that CPU                | 0 of 20000   |
| the same, with the `printf` subshell reniced to 19, pinned | 1802 of 2000 |
| the same, reniced, not pinned                              | 1 of 2000    |

## The fix

Give `head` its input without a pipe, so there is no writer to kill:

```diff
-    input2_out=$(printf "%s" "$input2" | head -n1)
+    input2_out=$(head -n1 <<< "$input2")
```

Line 16 needs no change: `tail` reads to the end. With the fix, all 65
schedules pass:

```console
$ rewind check --epoch 1791936000 --all 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-git-gc-closure-fixed'
schedule   0: exited:0            83827 steps  a50a5ab6d992  run f407cd32006aa2f5
schedule   1: exited:0            94523 steps  a50a5ab6d992  run 51308cc3c42ede99
...
0 of 64 other schedules ended differently
same result under all 65 schedules
```

## The recording

Both files are in the
[case-studies release](https://github.com/fzakaria/rewindvm/releases/tag/case-studies):

- [nix-gc-closure-sigpipe.rwd](https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-gc-closure-sigpipe.rwd)
  (222.9 KB), the trace, for `rewind events`, `rewind log` and the app; the
  [viewer](https://rewindvm.dev/viewer/?url=examples/nix-gc-closure.rwd) opens it in a browser.
- [nix-gc-closure-sigpipe-replayable.rwd](https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-gc-closure-sigpipe-replayable.rwd)
  (535.4 MB), with the kernel, input image and keyframes, for `rewind replay`
  and `rewind shell` on an AMD Zen 2 or later.

```console
$ rewind import https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-gc-closure-sigpipe-replayable.rwd
$ rewind replay e413b6df
$ rewind shell e413b6df <step>
$ rewind-app https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-gc-closure-sigpipe.rwd
```

The run ids above come from the derivations in the
[example flake](../../examples/case-studies/flake.nix), on nixpkgs
[b4fd65b1](https://github.com/NixOS/nixpkgs/commit/b4fd65b198c599cbe814fcb9f42d25d021595ec9),
at the epoch the commands pass. Any change to a derivation gives a different
set of runs.
