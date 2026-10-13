# Tutorial: say what a correct run is

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

```console
$ rewind check --epoch 1790985600 --schedules 8 --property 'atomic-replace=*.conf' github:fzakaria/rewindvm#config-reload
rewind: packing 62 store paths for config-reload-0.1.0
schedule   0: exited:0       4563 steps  f7b1c7741aba  run d83eda44b1eee197  violates atomic-replace
  atomic-replace: step 3238: process 139 (config) truncated /build/config-reload/app.conf in place, which the run wrote at step 3233; a reader can see it empty or half written
...
schedule   8: exited:2       3778 steps    run 5ed6470d27365add  violates atomic-replace
every schedule violated a property, as schedule 0 did
open it at the step: rewind open d83eda44b1eee197 3238
```

The same file has a property for the failure itself. The VM's kernel records
an open for reading only when some process has the file open for writing,
the moment a reader can see it half written, and
`no-read-while-written` fails a run where that happens:

```console
$ rewind check --epoch 1790985600 --property 'no-read-while-written=*.conf' github:fzakaria/rewindvm#config-reload | grep -E 'decides it|failing run'
step 3274 decides it: a reschedule there makes the run violate no-read-while-written
failing run violates no-read-while-written: step 3285: process 140 (config) opened /build/config-reload/app.conf for reading while process 139 (config), which opened it for writing at step 3274, had not closed it
```

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

```console
$ rewind check --epoch 1790985600 --schedules 0 github:fzakaria/rewindvm#bank
...
schedule   0: exited:0       4558 steps  e9eff8d15d00  run 21cf5e28fa804c35  holds
never true in any run: sometimes "deposits-overlapped"
every property held under the one schedule tried
```

No run made two deposits overlap, so no run could lose one. With its 64
schedules, check finds the run that does:

```console
$ rewind check --epoch 1790985600 github:fzakaria/rewindvm#bank | grep -E 'decides it|passing:|failing:|failing run violates sdk'
step 3258 decides it: a reschedule there makes the run violate same-outcome, sdk
passing: run 21cf5e28fa804c35, schedule 0
failing: run 1cabfbed15ad58ec, schedule 1 over steps 3258..3259
failing run violates sdk: step 3298: always "balance-adds-up" failed: balance 380, expected 400
```

The SDK is a line written to `/dev/rewind` per call, and outside Rewind every
call does nothing, so the calls can stay in the program.
[sdk](https://github.com/fzakaria/rewindvm/tree/main/sdk) has it for C, Rust,
Go, Python with a pytest plugin, Java with a JUnit extension, and shell.

## Vary the randomness

`hash-order` tests a function that collects tags through a Rust `HashSet`,
and expects them in one order. Rust seeds each `HashSet` from the system's
randomness, which in the VM is a function of `--seed`, so no schedule
changes the order and every one passes. `--seeds` tries other seeds:

```console
$ rewind check --epoch 1790985600 --seeds 8 --schedules 0 github:fzakaria/rewindvm#hash-order | grep -E '^seed|decides it|passing:|failing:'
rewind: packing 67 store paths for hash-order-0.1.0
seed  0 schedule   0: exited:0       4811 steps  83f239c2e313  run 4274b5643f737d4b
seed  1 schedule   0: exited:2       3557 steps    run b77e3df778b90b00
seed 1 decides it: unperturbed, seed 0 passes and seed 1 fails
passing: run 4274b5643f737d4b, seed 0, schedule 0
failing: run b77e3df778b90b00, seed 1, schedule 0
```

Seed 1 orders the tags the other way. Every seed is tried
unperturbed and then under `--schedules` perturbed schedules, so one search
covers both.

## In the desktop app

Open the failing `bank` run beside the passing one, at the assertion:

```console
$ rewind open 1cabfbed 3298 --compare 21cf5e28
```

![The app zoomed in around step 3298 of the failing bank run: a red mark above the track where always "balance-adds-up" failed, the test's span under the track in red, and the event card naming it](../site/img/app-assertion.png)

The red mark above the track is the assertion that broke, and pointing at it
says what broke. The event card names it, and Jump to failure goes to it.
`bank` marks its test with the SDK too, as the pytest and JUnit plugins mark
theirs, and the strip under the track has a span per test, green passed and
red failed.

The chevron beside Fork from here also offers Check from here with 8 seeds
of 8 schedules, each seed after the run's own reseeding a fork of it at the
playhead. From the start of `hash-order`'s passing run, a row of cells per
seed, blue where the seed made the test fail:

![The app after Check from here with 8 seeds of 8 schedules at the start of hash-order's passing run: a card with a row of cells per seed, the seeds that made the test fail in blue](../site/img/app-seeds.png)

## What to read next

- [The advanced tutorial](tutorial-advanced.md): a property as a script, a
  file of properties, and seeds from a step of a recorded run.
- [The SDK](https://github.com/fzakaria/rewindvm/tree/main/sdk): what each
  call is for, in every language.
- [Design](design.md#exploring-interleavings): how the search and narrowing
  work.
