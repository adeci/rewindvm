# Case study: a SIGPIPE in Nix's gc-closure test

Nix's functional test `gc-closure.sh` can fail with exit status 141. Rewind VM
failed it on the first run; the host passed it 200 times. We reported it as
[NixOS/nix#16546](https://github.com/NixOS/nix/issues/16546), and the fix
below was merged as [NixOS/nix#16547](https://github.com/NixOS/nix/pull/16547).

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
([flake](../../examples/case-studies/flake.nix)). It fails on schedule 0:

```console
$ rewind nix 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-git-gc-closure'
rewind: packing 217 store paths for nix-functional-gc-closure-2.36pre20260912_203f85b2
...
+(gc-closure.sh:45) nix_gc_closure false
...
++(gc-closure.sh:14) nix build -f dependencies2.nix input2_drv --no-link --print-out-paths
...
+(gc-closure.sh:14) input2=$'/build/nix-test/main/gc-closure/store/ij8qwx0m6dvpmggqmq8zmhbaiw6ym8pg-dependencies-input-2\n/build/nix-test/main/gc-closure/store/hw6dm6...
++(gc-closure.sh:15) printf %s $'/build/nix-test/main/gc-closure/store/ij8qwx0m6dvpmggqmq8zmhbaiw6ym8pg-dependencies-input-2\n/build/nix-test/main/gc-closure/store/hw...
++(gc-closure.sh:15) head -n1
+(gc-closure.sh:15) input2_out=/build/nix-test/main/gc-closure/store/ij8qwx0m6dvpmggqmq8zmhbaiw6ym8pg-dependencies-input-2
...
1/1 main - nix-functional-tests:gc-closure FAIL             0.26s   (exit status 141 or signal 13 SIGPIPE)
...
rewind: run d6deb54e63ee7a3b exited:1 after 37507 steps, 1.436s virtual, 6.449s wall (poweroff)

$ rewind replay d6deb54e
identical: 9809 events over 37507 steps
```

`input2_out` got the right value, and the test still died of signal 13.

## What died

The events around the failure, and the processes just before it:

```console
$ rewind events d6deb54e --from 37240 --to 37340
     37250   359/359   fork() = 416
     37253   416/416   fork() = 417
     37256   416/416   fork() = 418
     37286   418/418   execve("/nix/store/2gfxiwls9hbgwdwcy43mprchwsq36mg6-coreutils-9.11/bin/head", ["head", "-n1"])
     37309   418/418   exit_group(head) exited:0
     37315   417/417   SIGPIPE code=0 addr=0x0
     37316   417/417   exit_group(bash) killed:SIGPIPE
     37318   416/416   SIGCHLD code=1 addr=0x0
     37319   416/416   exit_group(bash) exited:141
     37322   359/359   SIGCHLD code=1 addr=0x0
     37335   359/359   exit_group(bash) exited:141
     37339   358/358   close("anon_inode:[pidfd]")

$ rewind ps d6deb54e 37292
...
   359       /nix/store/10dxp0qxqxxsyiljrh2kp0xqhz6arhcx-bash-5.3p15/bin/bash -x -e -u -o pipefail gc-closure.sh
   416         /nix/store/10dxp0qxqxxsyiljrh2kp0xqhz6arhcx-bash-5.3p15/bin/bash -x -e -u -o pipefail gc-closure.sh (fork)
   417           /nix/store/10dxp0qxqxxsyiljrh2kp0xqhz6arhcx-bash-5.3p15/bin/bash -x -e -u -o pipefail gc-closure.sh (fork)
   418           head -n1
```

416 is the command substitution, 417 runs `printf` and 418 runs `head`. `head`
exits 0, then 417 dies of SIGPIPE and the test exits 141.

## Count the writes

`rewind cat` reads a file in a fork of the run at any step. `/proc/<pid>/io`
counts each process's writes:

```console
$ for step in 37305 37309 37315; do
>   echo "step $step"
>   rewind cat d6deb54e $step /proc/417/io | grep -E '^(wchar|syscw)' | sed 's/^/  417 /'
>   rewind cat d6deb54e $step /proc/418/io | grep -E '^(wchar|syscw)' | sed 's/^/  418 /'
> done
step 37305
  417 wchar: 224
  417 syscw: 1
  418 wchar: 30
  418 syscw: 1
step 37309
  417 wchar: 316
  417 syscw: 2
  418 wchar: 122
  418 syscw: 2
step 37315
  417 wchar: 316
  417 syscw: 3
rewind: /proc/418/io: no such file at this step
```

The first write of each is its `bash -x` trace line. By 37309 `printf` has
written the first store path (92 bytes) and `head` has echoed it and exited.
At 37315 `printf`'s third write adds no bytes: the pipe has no reader left.

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
$ rewind threads d6deb54e --from 37284 --to 37320
     37284      37284       358/358  meson
     37285      37285       418/418  bash
     37286      37301       418/418  head
     37302      37306       417/417  bash
     37307      37307     kernel 11  ksoftirqd/0
     37308      37309       418/418  head
     37310      37310     kernel 11  ksoftirqd/0
     37311      37312       359/359  bash
     37313      37313       416/416  bash
     37314      37314     kernel 11  ksoftirqd/0
     37315      37316       417/417  bash
     37317      37317     kernel 11  ksoftirqd/0
     37318      37319       416/416  bash
     37320      37320     kernel 11  ksoftirqd/0
```

`head` blocks in `read` by 37301. `printf` makes its first write at 37302,
which wakes `head`. `head` runs at 37308 and exits, and `printf` only gets the
CPU back at 37315.

## How often it fails

```console
$ rewind check --all --schedules 256 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-git-gc-closure'
schedule   0: exited:1      37507 steps    run d6deb54e63ee7a3b
schedule   1: exited:0      94453 steps  a50a5ab6d992  run 801b25958028b8e7
...
schedule  41: exited:1      43328 steps    run f3a81773fc0c51e5
...
schedule 214: exited:1      84959 steps    run efad69daaa21e8b5
...
schedule 0 failed; 254 of 256 perturbed schedules ended differently

schedule 1 passes where schedule 0 fails; narrowing the steps it perturbs
perturbing only steps 37303..37872 still ends differently
step 37871 decides it: a timer 49.9 µs late there makes the run pass

passing: run 9ab082205f1b42d3, schedule 1 over steps 37303..37872
failing: run 2b1a9ab1f5e313b8, schedule 1 over steps 37303..37871
the two are the same run until step 37871

where /nix/store/10dxp0qxqxxsyiljrh2kp0xqhz6arhcx-bash-5.3p15/bin/bash -x -e -u -o pipefail gc-closure.sh first behaves differently:
  both          74565   359/359   fork() = 534
  both          74568   534/534   fork() = 535
  both          74571   534/534   fork() = 536
  failing       74630   535/535   SIGPIPE code=0 addr=0x0
  failing       74631   535/535   exit_group(bash) killed:SIGPIPE
  failing       74633   534/534   SIGCHLD code=1 addr=0x0
  failing       74634   534/534   exit_group(bash) exited:141
  passing       74578   535/535   exit_group(bash) exited:0
  passing       74609   534/534   SIGCHLD code=1 addr=0x0
  passing       74610   534/534   exit_group(bash) exited:0
  passing       74612   359/359   SIGCHLD code=1 addr=0x0

open both in the desktop app: rewind open 2b1a9ab1f5e313b8 74630 --compare 9ab082205f1b42d3
```

Schedules 0, 41 and 214 fail; the other 254 pass. Here the unperturbed
schedule happens to be the failing order. The test runs the pipeline three
times, so the narrowed pair parts at a later call, step 74630.

Perturbing only from step 37253, where the pipeline forks `printf`, moves 62
of 64 schedules off the failing order:

```console
$ rewind check --run d6deb54e --schedule-from 37253 --schedules 64 --all --no-narrow
schedule   0: exited:1      37507 steps    run d6deb54e63ee7a3b
schedule   1: exited:0      88436 steps  a50a5ab6d992  run 6ab3f5362d555a76
schedule   2: exited:0      88329 steps  a50a5ab6d992  run 682fe8225b49e35b
...
schedule  30: exited:1      37529 steps    run e416ba8b839efea7
...
schedule  36: exited:1      37536 steps    run 0326b5b03deeac5a
...
schedule  64: exited:0      88360 steps  a50a5ab6d992  run 47af11d7e52a9cc2
run d6deb54e63ee7a3b failed; 62 of 64 perturbed schedules ended differently
```

Schedules 30 and 36 fail the same way, in the same pipeline: `head` exits
between `printf`'s two writes, and `printf` dies of SIGPIPE at step 37330 and 37333.

Nix 2.35.2 from nixpkgs has the same line. It fails schedule 0 too, and 5 of
its 1025 runs fail, all with SIGPIPE:

```console
$ rewind check --all --schedules 1024 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-gc-closure'
...
schedule   0: exited:1      36645 steps    run 923e38ffd8c63851
...
schedule 380: exited:1      42327 steps    run ecd1ae341b746262
...
schedule 598: exited:1      64405 steps    run ec597c4d82c5cf3f
...
schedule 843: exited:1      82981 steps    run e164c79070f367cd
...
schedule 894: exited:1      82957 steps    run 836489d5467dfbec
...
schedule 0 failed; 1020 of 1024 perturbed schedules ended differently
```

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
$ rewind check --all 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-git-gc-closure-fixed'
schedule   0: exited:0      83722 steps  a50a5ab6d992  run e6deae8939c0328e
schedule   1: exited:0      94498 steps  a50a5ab6d992  run ea7dfcb0b2d8cdfe
...
0 of 64 perturbed schedules ended differently
same result under all 65 schedules
```

## The recording

Both files are in the
[case-studies release](https://github.com/fzakaria/rewindvm/releases/tag/case-studies):

- [nix-gc-closure-sigpipe.rwd](https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-gc-closure-sigpipe.rwd)
  (94.1 KB), the trace, for `rewind events`, `rewind log` and the app.
- [nix-gc-closure-sigpipe-replayable.rwd](https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-gc-closure-sigpipe-replayable.rwd)
  (535.0 MB), with the kernel, input image and keyframes, for `rewind replay`
  and `rewind shell` on an AMD Zen 2 or later.

```console
$ rewind import https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-gc-closure-sigpipe-replayable.rwd
$ rewind replay d6deb54e
$ rewind shell d6deb54e <step>
$ rewind-app https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-gc-closure-sigpipe.rwd
```

The run ids above come from the derivations in the
[example flake](../../examples/case-studies/flake.nix), on nixpkgs
[b4fd65b1](https://github.com/NixOS/nixpkgs/commit/b4fd65b198c599cbe814fcb9f42d25d021595ec9).
Any change to a derivation gives a different set of runs.
