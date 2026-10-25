# Case study: a hang in Nix's store schema migration

A Nix master build from April 2026 could hang when several `nix` commands
opened a new store at once, printing `SQLite database ... is busy` every ten
seconds. It was reported as
[NixOS/nix#15693](https://github.com/NixOS/nix/issues/15693) and fixed upstream
in two steps, not by us. Rewind VM reproduced it, gdb in the VM pinned the
hang to `SQLITE_BUSY_SNAPSHOT`, and checks showed why the first fix,
[#15694](https://github.com/NixOS/nix/pull/15694), left the hang in place and
the second, [#15967](https://github.com/NixOS/nix/pull/15967), removed it.

Scrub the recording in your browser: [open it in the viewer](https://rewindvm.dev/viewer/?url=examples/nix-schema-migration-hang.rwd).

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
inputs ([flake](../../examples/case-studies/flake.nix)). `--epoch` fixes the
VM's clock, which seeds its randomness, so these are the runs you get:

```console
$ rewind check --epoch 1791936000 --all --schedules 256 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-concurrent-builds-15693-gdb'
rewind: packing 260 store paths for nix-functional-concurrent-builds-gdb-2.35.0pre20260414_a94dee9
schedule   0: exited:0            34629 steps  a50a5ab6d992  run d07f3f1b61cd4329
schedule   1: exited:0            49268 steps  a50a5ab6d992  run 639e82ab22e6183c
...
schedule   4: exited:1           411958 steps    run e9f40610b8f3f70c
...
schedule  19: exited:1           412617 steps    run f6ef0e07caf642e9
...
schedule  66: exited:1           411913 steps    run 001306b737db571a
...
schedule 168: exited:1           560832 steps    run 5b9b20b81aa33cec
...
schedule 174: exited:1           410589 steps    run 0450b3777e43dc13
...
schedule 250: exited:1           413625 steps    run ac94f3527a2b4fbb
...
6 of 256 other schedules ended differently

schedule 4 ends differently from schedule 0; finding the step that decides it
schedule 4 limited to steps 303..26517 still ends differently
step 26516 decides it: a request to switch threads there makes the run fail

passing: run fa9e7202a96db819, schedule 4 over steps 303..26516
failing: run e296601ffb6deb66, schedule 4 over steps 303..26517
the two are the same run until step 26516

where /nix/store/...-bash-5.3p3/bin/bash -x -e -u -o pipefail concurrent-builds.sh first behaves differently:
  both          22896   188/188   fork() = 223
  both          22899   188/188   fork() = 224
  both          22902   188/188   fork() = 225
  failing      267631   188/188   SIGTERM code=0 addr=0x0
  failing      267632   188/188   exit_group(bash) killed:SIGTERM
  passing       34441   188/188   SIGCHLD code=1 addr=0x0
  passing       36064   188/188   SIGCHLD code=1 addr=0x0
  passing       37105   188/188   SIGCHLD code=1 addr=0x0
  passing       37106   188/188   exit_group(bash) exited:0

open both in the desktop app: rewind open e296601ffb6deb66 267631 --compare fa9e7202a96db819
```

Schedule 0 passes and 6 of the other 256 fail. The narrowed window
stays wide because the six `nix` processes race from the moment they start. On the host the test passed 51 of
51 runs.

## What the failing run shows

Schedule 174 ends with meson's 300 second timeout, reached in seconds
of wall time since the VM skips ahead while it sleeps:

```console
$ rewind log 0450b377 | grep -E 'UNIQUE|is busy|TIMEOUT' | sort | uniq -c
      1 1/1 nix-functional-tests:ca / concurrent-builds TIMEOUT        300.02s   killed by signal 15 SIGTERM
      1 1/1 nix-functional-tests:ca / concurrent-builds        TIMEOUT        300.02s   killed by signal 15 SIGTERM
      2        insert into SchemaMigrations values('20251017-ca-derivations')': constraint failed, UNIQUE constraint failed: SchemaMigrations.migration (in '/build/nix-test/ca/concurrent-builds/var/nix/db/db.sqlite')
     28 warning: SQLite database '/build/nix-test/ca/concurrent-builds/var/nix/db/db.sqlite' is busy

$ rewind events 0450b377 | grep -E 'exit_group\(nix\)'
     29089   223/223   exit_group(nix) exited:1
     29430   224/224   exit_group(nix) exited:1
     33059   238/238   exit_group(nix) killed:SIGTERM
     39077   222/222   exit_group(nix) exited:0
     42112   221/221   exit_group(nix) exited:0
     49053   225/225   exit_group(nix) exited:0
    410403   220/220   exit_group(nix) exited:1
```

Both failures from the issue are in this run. 223 and 224 lost the race to
record a migration and exited with the UNIQUE error. 220 hung until meson
killed the test at step 410403. (238 is the build hook, which often
ends with SIGTERM in passing runs too.)

## Where the hung process is

At step 200000 only 220 is left, and it holds every lock:

```console
$ rewind ps 0450b377 200000
     1 /init
    34   bash -e /nix/store/...-source-stdenv.sh /nix/store/...-default-builder.sh
   187     /nix/store/...-python3-3.13.11/bin/python3.13 /nix/store/...-meson-1.9.1/bin/meson test --no-rebuild --print-errorlogs concurrent-builds
   188       /nix/store/...-bash-5.3p3/bin/bash -x -e -u -o pipefail concurrent-builds.sh
   220         nix build --no-link --file ./racy.nix
   227           (thread)

$ rewind cat 0450b377 200000 /proc/locks
1: POSIX  ADVISORY  READ 220 00:03:1812 124 124
2: POSIX  ADVISORY  READ 220 00:03:1812 128 128
3: POSIX  ADVISORY  READ 220 00:03:1809 1073741826 1073742335
4: FLOCK  ADVISORY  READ 220 00:03:1808 0 EOF
```

220 holds the big lock shared (inode 1808), SQLite's lock on the
database (1809) and a read mark in the WAL index (1812). It is
inside a read transaction, and nothing else is in its way.

## Ask gdb

`rewind shell` opens a shell in a throwaway fork of the run at a step. gdb
attaches to 220 there and catches its next retry. The shell reads these
lines from `gdb.sh`; typed in, they do the same:

```console
$ rewind shell 0450b377 200000 --pid 220 < gdb.sh
rewind: a shell at step 200000 of 0450b3777e43dc13; exit it to leave
[rewind] /build/source/tests/functional/ca # cat > /tmp/g.cmd <<'END'
> set pagination off
> handle SIGSTOP nostop noprint nopass
> set unwind-on-signal on
> bt
> break sqlite3_exec
> continue
> set $db = $rdi
> printf "statement: %.200s\n", (char *) $rsi
> finish
> printf "sqlite3_exec returned %d\n", (int) $rax
> printf "extended error code %d\n", (int) sqlite3_extended_errcode($db)
> printf "errmsg %s\n", (char *) sqlite3_errmsg($db)
> printf "autocommit %d\n", (int) sqlite3_get_autocommit($db)
> detach
> END
[rewind] /build/source/tests/functional/ca # gdb -p 220 -batch -x /tmp/g.cmd 2>&1 | grep -vE "^warning|auto-load|add-auto-load|line to your|To (enable|completely)|set auto-load|For more information|info .\(gdb\)|^$"
[New LWP 227]
...
#0  0x00007fc4f928e223 in clock_nanosleep@GLIBC_2.2.5 () from /nix/store/...-glibc-2.40-218/lib/libc.so.6
#1  0x00007fc4f929a8e7 in nanosleep () from /nix/store/...-glibc-2.40-218/lib/libc.so.6
#2  0x00007fc4f9fe5c1a in nix::handleSQLiteBusy(nix::SQLiteBusy const&, long&) () from /nix/store/...-nix-store-2.35.0pre/lib/libnixstore.so.2.35.0
#3  0x00007fc4f9e8e8d5 in void nix::retrySQLite<void, nix::SQLite::exec(std::__cxx11::basic_string<char, std::char_traits<char>, std::allocator<char> > const&)::{lambda()#1}>(nix::SQLite::exec(std::__cxx11::basic_string< ...
#4  0x00007fc4f9f8c37e in nix::LocalStore::upgradeDBSchema(nix::LocalStore::State&)::{lambda(std::__cxx11::basic_string<char, std::char_traits<char>, std::allocator<char> > const&, std::__cxx11::basic_string<char, std::c ...
#5  0x00007fc4f9f8e8a9 in nix::LocalStore::upgradeDBSchema(nix::LocalStore::State&) () from /nix/store/...-nix-store-2.35.0pre/lib/libnixstore.so.2.35.0
#6  0x00007fc4f9f8f605 in nix::LocalStore::LocalStore(nix::ref<nix::LocalStoreConfig const>) () from /nix/store/...-nix-store-2.35.0pre/lib/libnixstore.so.2.35.0
#7  0x00007fc4f9f89046 in nix::LocalStoreConfig::openStore() const () from /nix/store/...-nix-store-2.35.0pre/lib/libnixstore.so.2.35.0
...
Breakpoint 1 at 0x7fc4f84a5a68
Thread 1 "nix" hit Breakpoint 1, 0x00007fc4f84a5a68 in sqlite3_exec () from /nix/store/...-sqlite-3.50.4/lib/libsqlite3.so
statement: drop index if exists IndexReferrer;
insert into SchemaMigrations values('20260309-drop-redundant-indexreferrer')
...
sqlite3_exec returned 5
extended error code 517
errmsg database is locked
autocommit 0
[Inferior 1 (process 220) detached]
[rewind] /build/source/tests/functional/ca # exit
```

220 is retrying the second migration. SQLite returns `SQLITE_BUSY` (5)
with the extended code 517, `SQLITE_BUSY_SNAPSHOT`, and `autocommit 0` says
the connection is still inside the migration's transaction.

## Root cause

`SQLITE_BUSY_SNAPSHOT`
([SQLite docs](https://www.sqlite.org/rescode.html#busy_snapshot)) is what a WAL connection gets when it tries to write after another connection has
committed since its read. Its snapshot stays stale until the transaction ends.

220 began the migration and read the schema, which fixed its snapshot.
Another `nix` process committed a migration. 220's write was refused, and
`SQLite::exec` retried the statement inside the same transaction, on the same
stale snapshot, forever: 28 warnings, ten seconds apart. A process whose
read came before the other commit but whose snapshot was still current ran
the migration again and failed with the UNIQUE error instead.

## Check the two fixes

[#15694](https://github.com/NixOS/nix/pull/15694) changed the insert to
`insert or ignore`. That removes the UNIQUE error, but not the stale snapshot.
At its merge commit
[c390460cd](https://github.com/NixOS/nix/commit/c390460cdf7ee8b3208d982e09f91555f980759e):

```console
$ rewind check --epoch 1791936000 --all --schedules 256 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-concurrent-builds-15694'
rewind: packing 216 store paths for nix-functional-concurrent-builds-2.35.0pre20260416_c390460
schedule   0: exited:0            35477 steps  a50a5ab6d992  run b1e28762428cddd5
...
schedule  19: exited:1           415467 steps    run e46d2a628311d089
...
schedule  43: exited:1           415889 steps    run a449bc35b2f02eb0
...
schedule  52: exited:1           412223 steps    run b335d232ced480da
...
schedule  85: exited:1           410672 steps    run 89302b96f97a7b2f
...
schedule  90: exited:1           418475 steps    run 8217613ca9e4cbe5
...
schedule  99: exited:1           413457 steps    run ad0441f1f357846b
...
schedule 121: exited:1           415321 steps    run 94ee005d249f2861
...
schedule 186: exited:1           414674 steps    run a1e6d0118ae4e0ae
...
8 of 256 other schedules ended differently
...
```

```console
$ rewind log e46d2a62 | grep -E 'UNIQUE|is busy|TIMEOUT' | sort | uniq -c
      1 1/1 nix-functional-tests:ca / concurrent-builds TIMEOUT        300.02s   killed by signal 15 SIGTERM
      1 1/1 nix-functional-tests:ca / concurrent-builds        TIMEOUT        300.02s   killed by signal 15 SIGTERM
     28 warning: SQLite database '/build/nix-test/ca/concurrent-builds/var/nix/db/db.sqlite' is busy
```

All 8 failing runs hang the same way, with no UNIQUE error.
[#15967](https://github.com/NixOS/nix/pull/15967) takes the big lock
exclusively while a migration runs, so no other process can commit during its
transaction. Nix 2.35.2 from nixpkgs has both fixes and passes every schedule:

```console
$ rewind check --epoch 1791936000 --all --schedules 256 'github:fzakaria/rewindvm?dir=examples/case-studies#nix-concurrent-builds'
rewind: packing 217 store paths for nix-functional-concurrent-builds-2.35.2
schedule   0: exited:0            38109 steps  a50a5ab6d992  run 2830edf2f50158ca
...
0 of 256 other schedules ended differently
same result under all 257 schedules
```

## The recording

Both files are in the
[case-studies release](https://github.com/fzakaria/rewindvm/releases/tag/case-studies):

- [nix-schema-migration-hang.rwd](https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-schema-migration-hang.rwd)
  (112.2 KB), the trace, for `rewind events`, `rewind log` and the app; the
  [viewer](https://rewindvm.dev/viewer/?url=examples/nix-schema-migration-hang.rwd) opens it in a browser.
- [nix-schema-migration-hang-replayable.rwd](https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-schema-migration-hang-replayable.rwd)
  (543.0 MB), with the kernel, input image and keyframes, for `rewind replay`
  and `rewind shell` on an AMD Zen 2 or later.

```console
$ rewind import https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-schema-migration-hang-replayable.rwd
$ rewind replay 0450b377
$ rewind shell 0450b377 <step>
$ rewind-app https://github.com/fzakaria/rewindvm/releases/download/case-studies/nix-schema-migration-hang.rwd
```
