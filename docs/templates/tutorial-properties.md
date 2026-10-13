# Tutorial: say what a correct run is

<!-- let epoch = 1790985600 -->
<!-- let repo = github:fzakaria/rewindvm -->

`rewind check` tries a job under many thread schedules and looks for one
that ends differently from schedule 0. This tutorial tells it what a correct
run is instead: properties it judges every run by, assertions the program
makes about itself, and seeds that vary the program's randomness as well as
its schedule. It uses three small examples from
[examples](https://github.com/fzakaria/rewindvm/tree/main/examples) and the
install from the [Nix tutorial](tutorial-nix.md).

## A bug on schedule 0

`config-reload` rewrites its config in place, truncating it and writing its
two lines one at a time, while another process rereads it. Its own check
fails only when a read lands between the truncate and the last write.
`atomic-replace` judges what the program did rather than how it ended, so it
finds the bug on schedule 0, with no search:

```console run name=atomic show=:3,-3:
$ rewind check --epoch {{epoch}} --schedules 8 --property 'atomic-replace=*.conf' {{repo}}#config-reload
```

<!-- assert: grep -qE '^  atomic-replace: step [0-9]+: process [0-9]+ \(config\) truncated /build/config-reload/app.conf in place' {{out:atomic}} && grep -q '^every schedule violated a property, as schedule 0 did$' {{out:atomic}} -->

The same file has a property for the failure itself. The VM's kernel records
an open for reading only when some process has the file open for writing,
the moment a reader can see it half written, and
`no-read-while-written` fails a run where that happens:

```console run name=read
$ rewind check --epoch {{epoch}} --property 'no-read-while-written=*.conf' {{repo}}#config-reload | grep -E 'decides it|failing run'
```

<!-- assert: grep -qE 'failing run violates no-read-while-written: step [0-9]+: process [0-9]+ \(config\) opened /build/config-reload/app.conf for reading while process [0-9]+ \(config\)' {{out:read}} -->

`rewind check --help` lists the others: `no-crash` for a child that died of
SIGSEGV while a retry hid it, `no-orphans` for a process left running,
`single-writer` for two processes writing one file at once, `writes-within`
for a test writing outside its directory, `junit` for a test runner's
reports, and `script` for a program of your own.

## Assertions from inside the program

`bank` has two threads deposit into one balance without a lock. It includes
`rewind.h`, the Rewind SDK's header for C, and says two things about itself:

```c
/* Another teller stored between this one's read and store. */
REWIND_SOMETIMES(balance != seen, "deposits-overlapped");
...
REWIND_ALWAYS(balance == expected, "balance-adds-up", "balance %ld, expected %ld", balance,
	      expected);
```

`always` must hold every time. `sometimes` must be true in some run: it
checks the search, not the program. Checked under schedule 0 alone, `bank`
passes, and check says why that means little:

```console run name=alone show=-3:
$ rewind check --epoch {{epoch}} --schedules 0 {{repo}}#bank
```

<!-- assert: grep -q '^never true in any run: sometimes "deposits-overlapped"$' {{out:alone}} && grep -q '^every property held under the one schedule tried$' {{out:alone}} -->

No run made two deposits overlap, so no run could lose one. With its 64
schedules, check finds the run that does:

```console run name=bank
$ rewind check --epoch {{epoch}} {{repo}}#bank | grep -E 'decides it|passing:|failing:|failing run violates sdk'
```

<!-- capture bank_passing: passing: run (\w+) -->
<!-- capture bank_failing: failing: run (\w+) -->
<!-- capture assert_step: failing run violates sdk: step (\d+): -->
<!-- assert: grep -qE 'failing run violates sdk: step [0-9]+: always "balance-adds-up" failed: balance [0-9]+, expected 400' {{out:bank}} -->

The SDK is a line written to `/dev/rewind` per call, and outside Rewind every
call does nothing, so the calls can stay in the program.
[sdk](https://github.com/fzakaria/rewindvm/tree/main/sdk) has it for C, Rust,
Go, Python with a pytest plugin, Java with a JUnit extension, and shell.

## Vary the randomness

`hash-order` tests a function that collects tags through a Rust `HashSet`,
and expects them in one order. Rust seeds each `HashSet` from the system's
randomness, which in the VM is a function of `--seed`, so no schedule
changes the order and every one passes. `--seeds` tries other seeds:

```console run name=seeds
$ rewind check --epoch {{epoch}} --seeds 8 --schedules 0 {{repo}}#hash-order | grep -E '^seed|decides it'
```

<!-- capture deciding_seed: seed (\d+) decides it -->
<!-- assert: grep -qE '^seed [0-9]+ decides it: unperturbed, seed [0-9]+ passes and seed [0-9]+ fails$' {{out:seeds}} -->

Seed {{deciding_seed}} orders the tags the other way. Every seed is tried
unperturbed and then under `--schedules` perturbed schedules, so one search
covers both.

## In the desktop app

Open the failing `bank` run beside the passing one, at the assertion:

```console
$ rewind open {{bank_failing|short}} {{assert_step}} --compare {{bank_passing|short}}
```

<!-- screenshot site/img/app-assertion: {{home}}/runs/{{bank_failing}} --compare {{home}}/runs/{{bank_passing}} --step {{assert_step}} ;; wait 2 -->

![The app at step {{assert_step}} of the failing bank run: a red mark above the track where always "balance-adds-up" failed, and the event card naming it](../site/img/app-assertion.png)

The red mark above the track is the assertion that broke, and pointing at it
says what broke. The event card names it, and Jump to failure goes to it.
Tests a job marks with the SDK, or its pytest and JUnit plugins, show as a
strip under the track, a span per test, green passed and red failed.

## What to read next

- [The advanced tutorial](tutorial-advanced.md): a property as a script, a
  file of properties, and seeds from a step of a recorded run.
- [The SDK](https://github.com/fzakaria/rewindvm/tree/main/sdk): what each
  call is for, in every language.
- [Design](design.md#exploring-interleavings): how the search and narrowing
  work.
