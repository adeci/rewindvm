# Examples

## mylib

A small C thread pool with a shutdown race, for the Rewind VM tutorials.

`pool_shutdown` frees the job queue before joining the workers, and a worker
that has finished a job checks whether the pool is stopping without holding
the lock, then counts the job through the queue. When shutdown runs between
that check and the count, the worker writes through a null queue pointer and
`test_pool_shutdown` dies with SIGSEGV. On a 16 core laptop that happens in
roughly one build in ten.

```console
$ make -C mylib check                           # plain, on the host
$ nix build github:fzakaria/rewindvm#mylib     # as a derivation
$ docker build -t mylib -f mylib/Containerfile mylib   # as a container
```

The tutorials find the failing interleaving with `rewind check`, look at it
step by step, and check the fix:
[Nix](../docs/tutorial-nix.md) · [container](../docs/tutorial-container.md).

## philosophers

The dining philosophers. Five threads share five mutexes in a ring, and each
locks the one on its left, then the one on its right. When every thread holds
its left mutex at once, each waits for its neighbor forever. `make check`
gives dinner ten seconds and fails with timeout's status 124 when it
deadlocks. Locking the lower numbered fork first fixes it.

```console
$ make -C philosophers check                       # plain, on the host
$ nix build github:fzakaria/rewindvm#philosophers  # as a derivation
```

## bank

Two tellers deposit into one account from two threads. A deposit reads the
balance, writes a line to the ledger, then stores the balance plus the
deposit, with no lock, so a teller that runs between the other's read and
store has its deposit written over. `make check` fails when the balance
comes up short. Holding a mutex across the read and the store fixes it.
It asserts with the Rewind SDK, through a copy of
[`sdk/c/rewind.h`](../sdk/c/rewind.h): the balance adds up, and some run
should have two deposits overlap.

```console
$ make -C bank check                       # plain, on the host
$ nix build github:fzakaria/rewindvm#bank  # as a derivation
```

## bank-java

`bank` in Java, as a JUnit 5 suite: one teller, two tellers, and a new
account. The two-tellers test loses a deposit the same way, and fails its
`assertEquals`. The suite is marked with the Rewind SDK's JUnit extension,
through a copy of [`sdk/java`](../sdk/java), so `rewind check` sees each
test's start and result, and JUnit writes its XML reports to `reports/`
for the `junit` property.

```console
$ make -C bank-java check JUNIT=junit-platform-console-standalone.jar  # plain, on the host
$ nix build github:fzakaria/rewindvm#bank-java                         # as a derivation
```

## config-reload

One process rewrites a config file in place, truncating it and writing its
two lines one at a time, while another process rereads it. A read between
the truncate and the last write sees a config with a line missing, and
`make check` fails. On a multicore host that happens nearly every time;
on Rewind VM's one CPU only some schedules land the reader there. Writing
a temporary file and renaming it over the config fixes it.

```console
$ make -C config-reload check                       # plain, on the host
$ nix build github:fzakaria/rewindvm#config-reload  # as a derivation
```

## waiter

A parent forks a worker and waits for it to exit: its SIGCHLD handler sets
a flag, and the parent checks the flag, logs that it is waiting, then calls
`pause`. A worker that exits after the check and before the `pause` sends
its signal into that gap, the parent pauses anyway, and nothing wakes it.
`make check` gives it ten seconds and fails with timeout's status 124.
Blocking SIGCHLD around the check and waiting with `sigsuspend` fixes it.

```console
$ make -C waiter check                       # plain, on the host
$ nix build github:fzakaria/rewindvm#waiter  # as a derivation
```

## hash-order

A post's tags, each once and lowercased, collected through a Rust
`HashSet`, and a test that expects them in the order its author saw. A
`HashSet` keeps no order, and Rust seeds its hasher from the system's
randomness, so the test passes in some runs and fails in others. In Rewind
VM the randomness is the seed's, so `rewind check --seeds` finds a seed it
fails under. Sorting the tags fixes it.

```console
$ make -C hash-order check                       # plain, on the host
$ nix build github:fzakaria/rewindvm#hash-order  # as a derivation
```
