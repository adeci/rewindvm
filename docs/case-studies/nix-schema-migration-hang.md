# Case study: a hang in Nix's store schema migration

A Nix master build from April 2026 could hang when several `nix` commands
opened a new store at once, printing `SQLite database ... is busy` every ten
seconds. It was reported as
[NixOS/nix#15693](https://github.com/NixOS/nix/issues/15693) and fixed upstream
in two steps, not by us. Rewind VM reproduced it, gdb in the VM pinned the
hang to `SQLITE_BUSY_SNAPSHOT`, and checks showed why the first fix,
[#15694](https://github.com/NixOS/nix/pull/15694), left the hang in place and
the second, [#15967](https://github.com/NixOS/nix/pull/15967), removed it.

## The code

On a new store, every `nix` process runs the schema migrations while holding
the store's big lock in shared mode only, so several can migrate at once. At
[a94dee99e](https://github.com/NixOS/nix/blob/a94dee99e1805b1df24daefcdfa86a3d50c63685/src/libstore/local-store.cc#L566-L601),
the version the issue names, each migration runs in its own transaction:

```cpp
    auto doUpgrade = [&](const std::string & migrationName, const std::string & stmt) {
        if (schemaMigrations.contains(migrationName))
            return;

        debug("executing Nix database schema migration '%s'...", migrationName);

        SQLiteTxn txn(state.db);
        state.db.exec(stmt + fmt(";\ninsert into SchemaMigrations values('%s')", migrationName));
        txn.commit();

        schemaMigrations.insert(migrationName);
    };
```

`SQLite::exec`
([sqlite.cc](https://github.com/NixOS/nix/blob/a94dee99e1805b1df24daefcdfa86a3d50c63685/src/libstore/sqlite.cc#L130-L136))
retries its statement with no limit for as long as SQLite says the database is
busy.

## Run it

The test is
[ca/concurrent-builds.sh](https://github.com/NixOS/nix/blob/a94dee99e1805b1df24daefcdfa86a3d50c63685/tests/functional/ca/concurrent-builds.sh):
six `nix build` processes on an empty store. The derivation is Nix's
`nix-functional-tests` at a94dee99e, narrowed to this test, with gdb in its
inputs ([flake](../../examples/case-studies/flake.nix)):

```console
$ rewind check --all --schedules 256 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-concurrent-builds-15693-gdb'
schedule   0: exited:0      34643 steps  a50a5ab6d992  run c3fa5882ba0ac81d
schedule   1: exited:0      36484 steps  a50a5ab6d992  run 42d89f5c9721f494
...
schedule  21: exited:1     685209 steps    run 206bf6e551e07b50
...
schedule  71: exited:1     412107 steps    run a66fa59200691f54
...
schedule  83: exited:1     697743 steps    run 3dc336cf3384290b
...
schedule 104: exited:1     410473 steps    run 06fd1acfd10f5ef1
...
schedule 206: exited:1     568822 steps    run d8bdb1b5cb779cc0
...
schedule 215: exited:1     412580 steps    run a0c8fec9b93389af
...
schedule 230: exited:1     410828 steps    run dcc8a0bbfa6c43f5
...
7 of 256 perturbed schedules ended differently

schedule 21 ends differently; narrowing the steps it perturbs
perturbing only steps 12773..23118 still ends differently
step 23117 decides it: a reschedule there makes the run fail

passing: run afa5b26cb911dae6, schedule 21 over steps 12773..23117
failing: run 27f6a5bbc7694573, schedule 21 over steps 12773..23118
the two are the same run until step 23117

where /nix/store/...-bash-5.3p3/bin/bash -x -e -u -o pipefail concurrent-builds.sh first behaves differently:
  both          20289   188/188   fork() = 224
  both          20292   188/188   fork() = 225
  both          25383   188/188   SIGCHLD code=1 addr=0x0
  failing       26734   188/188   SIGCHLD code=1 addr=0x0
  failing       31280   188/188   SIGCHLD code=1 addr=0x0
  failing      261154   188/188   SIGTERM code=0 addr=0x0
  failing      261155   188/188   exit_group(bash) killed:SIGTERM
  passing       32551   188/188   exit_group(bash) exited:0

open both in the desktop app: rewind open 27f6a5bbc7694573 26734 --compare afa5b26cb911dae6
```

Seven schedules fail. The narrowed window stays wide because the six `nix`
processes race from the moment they start. On the host the test passed 51 of
51 runs.

## What the failing run shows

Schedule 230 ends with meson's 300 second timeout, reached in seconds of wall
time since the VM skips ahead while it sleeps:

```console
$ rewind log dcc8a0bb | grep -E 'UNIQUE|is busy|TIMEOUT' | sort | uniq -c
      1 1/1 nix-functional-tests:ca / concurrent-builds TIMEOUT        300.03s   killed by signal 15 SIGTERM
      1 1/1 nix-functional-tests:ca / concurrent-builds        TIMEOUT        300.03s   killed by signal 15 SIGTERM
      2        insert into SchemaMigrations values('20251017-ca-derivations')': constraint failed, UNIQUE constraint failed: SchemaMigrations.migration (in '/build/nix-test/ca/concurrent-builds/var/nix/db/db.sqlite')
     28 warning: SQLite database '/build/nix-test/ca/concurrent-builds/var/nix/db/db.sqlite' is busy

$ rewind events dcc8a0bb | grep -E 'exit_group\(nix\)'
     26782   224/224   exit_group(nix) exited:1
     27151   222/222   exit_group(nix) exited:1
     32727   238/238   exit_group(nix) killed:SIGTERM
     37423   220/220   exit_group(nix) exited:0
     40319   221/221   exit_group(nix) exited:0
     47358   225/225   exit_group(nix) exited:0
    410647   223/223   exit_group(nix) exited:1
```

Both failures from the issue are in this run. 224 and 222 lost the race to
record a migration and exited with the UNIQUE error. 223 hung until meson
killed the test at step 410647. (238 is the build hook, which often ends with
SIGTERM in passing runs too.)

## Where the hung process is

At step 200000 only 223 is left, and it holds every lock:

```console
$ rewind ps dcc8a0bb 200000
     1 /init
    34   bash -e /nix/store/...-source-stdenv.sh /nix/store/...-default-builder.sh
   187     /nix/store/...-python3-3.13.11/bin/python3.13 /nix/store/...-meson-1.9.1/bin/meson test --no-rebuild --print-errorlogs concurrent-builds
   188       /nix/store/...-bash-5.3p3/bin/bash -x -e -u -o pipefail concurrent-builds.sh
   223         nix build --no-link --file ./racy.nix
   228           (thread)

$ rewind cat dcc8a0bb 200000 /proc/locks
1: POSIX  ADVISORY  READ 223 00:03:1812 124 124
2: POSIX  ADVISORY  READ 223 00:03:1812 128 128
3: POSIX  ADVISORY  READ 223 00:03:1809 1073741826 1073742335
4: FLOCK  ADVISORY  READ 223 00:03:1808 0 EOF
```

223 holds the big lock shared (inode 1808), SQLite's lock on the database
(1809) and a read mark in the WAL index (1812). It is inside a read
transaction, and nothing else is in its way.

## Ask gdb

`rewind shell` opens a shell in a throwaway fork of the run at a step. gdb
attaches to 223 there and catches its next retry:

```console
$ rewind shell dcc8a0bb 200000 --pid 223
rewind: a shell at step 200000 of dcc8a0bbfa6c43f5; exit it to leave
[rewind] /build/source/tests/functional/ca # cat /tmp/g.cmd
set pagination off
handle SIGSTOP nostop noprint nopass
set unwind-on-signal on
bt
break sqlite3_exec
continue
set $db = $rdi
printf "statement: %.200s\n", (char *) $rsi
finish
printf "sqlite3_exec returned %d\n", (int) $rax
printf "extended error code %d\n", (int) sqlite3_extended_errcode($db)
printf "errmsg %s\n", (char *) sqlite3_errmsg($db)
printf "autocommit %d\n", (int) sqlite3_get_autocommit($db)
detach
[rewind] /build/source/tests/functional/ca # gdb -p 223 -batch -x /tmp/g.cmd 2>&1 | grep -vE "^warning|auto-load|add-auto-load|line to your|To (enable|completely)|set auto-load|For more information|info .\(gdb\)|^$"
[New LWP 228]
...
#0  0x00007f763de2f223 in clock_nanosleep@GLIBC_2.2.5 () from /nix/store/...-glibc-2.40-218/lib/libc.so.6
#1  0x00007f763de3b8e7 in nanosleep () from /nix/store/...-glibc-2.40-218/lib/libc.so.6
#2  0x00007f763eb86c1a in nix::handleSQLiteBusy(nix::SQLiteBusy const&, long&) () from /nix/store/...-nix-store-2.35.0pre/lib/libnixstore.so.2.35.0
#3  0x00007f763ea2f8d5 in void nix::retrySQLite<void, nix::SQLite::exec(...)::{lambda()#1}>(...) ...
#4  0x00007f763eb2d37e in nix::LocalStore::upgradeDBSchema(nix::LocalStore::State&)::{lambda(...)#1}::operator()(...) ...
#5  0x00007f763eb2f8a9 in nix::LocalStore::upgradeDBSchema(nix::LocalStore::State&) () from /nix/store/...-nix-store-2.35.0pre/lib/libnixstore.so.2.35.0
#6  0x00007f763eb30605 in nix::LocalStore::LocalStore(nix::ref<nix::LocalStoreConfig const>) () from /nix/store/...-nix-store-2.35.0pre/lib/libnixstore.so.2.35.0
#7  0x00007f763eb2a046 in nix::LocalStoreConfig::openStore() const () from /nix/store/...-nix-store-2.35.0pre/lib/libnixstore.so.2.35.0
...
Breakpoint 1 at 0x7f763d046a68
Thread 1 "nix" hit Breakpoint 1, 0x00007f763d046a68 in sqlite3_exec () from /nix/store/...-sqlite-3.50.4/lib/libsqlite3.so
statement: drop index if exists IndexReferrer;
insert into SchemaMigrations values('20260309-drop-redundant-indexreferrer')
...
sqlite3_exec returned 5
extended error code 517
errmsg database is locked
autocommit 0
[Inferior 1 (process 223) detached]
```

223 is retrying the second migration. SQLite returns `SQLITE_BUSY` (5) with
the extended code 517, `SQLITE_BUSY_SNAPSHOT`, and `autocommit 0` says the
connection is still inside the migration's transaction.

## Root cause

`SQLITE_BUSY_SNAPSHOT`
([SQLite docs](https://www.sqlite.org/rescode.html#busy_snapshot)) is what a WAL connection gets when it tries to write after another connection has
committed since its read. Its snapshot stays stale until the transaction ends.

223 began the migration and read the schema, which fixed its snapshot. Another
`nix` process committed a migration. 223's write was refused, and
`SQLite::exec` retried the statement inside the same transaction, on the same
stale snapshot, forever: 28 warnings, ten seconds apart. A process whose read
came before the other commit but whose snapshot was still current ran the
migration again and failed with the UNIQUE error instead.

## Check the two fixes

[#15694](https://github.com/NixOS/nix/pull/15694) changed the insert to
`insert or ignore`. That removes the UNIQUE error, but not the stale snapshot.
At its merge commit
[c390460cd](https://github.com/NixOS/nix/commit/c390460cdf7ee8b3208d982e09f91555f980759e):

```console
$ rewind check --all --schedules 256 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-concurrent-builds-15694'
schedule   0: exited:0      35485 steps  a50a5ab6d992  run edcd0776e323d905
...
schedule  79: exited:1     413417 steps    run ed86a5ebafba1ed2
...
schedule 187: exited:1     413406 steps    run e3e8ea11140c72f1
...
5 of 256 perturbed schedules ended differently

$ rewind log ed86a5eb | grep -E 'UNIQUE|is busy|TIMEOUT' | sort | uniq -c
      1 1/1 nix-functional-tests:ca / concurrent-builds TIMEOUT        300.04s   killed by signal 15 SIGTERM
      1 1/1 nix-functional-tests:ca / concurrent-builds        TIMEOUT        300.04s   killed by signal 15 SIGTERM
     28 warning: SQLite database '/build/nix-test/ca/concurrent-builds/var/nix/db/db.sqlite' is busy
```

All five failing runs hang the same way, with no UNIQUE error.
[#15967](https://github.com/NixOS/nix/pull/15967) takes the big lock
exclusively while a migration runs, so no other process can commit during its
transaction. Nix 2.35.2 from nixpkgs has both fixes and passes every schedule:

```console
$ rewind check --all --schedules 256 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-concurrent-builds'
schedule   0: exited:0      38124 steps  a50a5ab6d992  run b1c1bf1d6bd2524d
...
0 of 256 perturbed schedules ended differently
same result under all 257 schedules
```

## The recording

Both files are in the
[case-studies release](https://github.com/fzakaria/rewindvm/releases/tag/case-studies):

- [nix-schema-migration-hang.rwd](https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-schema-migration-hang.rwd)
  (50.8 KB), the trace, for `rewind events`, `rewind log` and the app.
- [nix-schema-migration-hang-replayable.rwd](https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-schema-migration-hang-replayable.rwd)
  (542.8 MB), with the kernel, input image and keyframes, for `rewind replay`
  and `rewind shell` on an AMD Zen 2 or later.

```console
$ rewind import https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-schema-migration-hang-replayable.rwd
$ rewind replay dcc8a0bb
$ rewind shell dcc8a0bb <step>
$ rewind-app https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-schema-migration-hang.rwd
```
