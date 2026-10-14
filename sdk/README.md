# The Rewind SDK

`rewind check` runs a job under many schedules and seeds and looks for one
that goes wrong. On its own it can only see how a run ended and what its
processes did. The SDK lets the program say what "wrong" means from the
inside: a balance that must add up, a cache that must never return a
stale value, which test is running.

Each call writes one line to `/dev/rewind`. The trace keeps the line with
the process, thread and step that wrote it, and `rewind check` judges
every run by them. Outside Rewind there is no `/dev/rewind` and every call
does nothing, so the calls can stay in production code.

## The calls

`always(condition, id)` says the condition holds every time the program
gets here. A run where it is false once violates the `sdk` property, at
the step it was false. Use it for invariants: the ledger balances, the
queue never holds more than its capacity.

`sometimes(condition, id)` says some run should make the condition true
here. It never fails a run. After the search, check counts the runs that made
each one true and lists each one no run made true. Use it to check the search itself, on a state that is legal but
risky: two tellers mid-deposit at once, a read racing a cache's
invalidation. `always` says the program handled that state, and
`sometimes` says the search reached it. If no run did, "no failure found"
says nothing about that case, and you need more schedules or a different
test. Mark the risky state rather than the bug: a condition true only when
the bug happens is never true once the bug is fixed.

`unreachable(id)` says no run should get here. A run that does violates
the `sdk` property at that step. Use it for branches that mean a bug: a
retry after shutdown, a fallback that should never trigger.

`test_begin(name)` and `test_end(name, result)` mark a test's boundaries
and result. A test that passed in schedule 0 and fails, or never
finishes, in another run violates the `sdk` property. The pytest plugin
and the JUnit 5 extension make these calls for you.

An id has no spaces. A message, where a call takes one, is shown when the
call fails. Each library writes a pass or a hit once, and a failure or a
miss once, per id and process, since every write costs the VM an exit.

`rewind check` judges the `sdk` property by itself when the job writes any
of these lines.

## Getting a library

Each library is one or two files with no dependencies, under the MIT
license. Install it from this repository, or copy the file into your own.

| Language | Get it                                                                           | Use it                                                             |
| -------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| C, C++   | copy [`c/rewind.h`](c/rewind.h)                                                  | `REWIND_ALWAYS(cond, "id", "fmt", ...)`                            |
| Rust     | `rewind-sdk = { git = "https://github.com/fzakaria/rewindvm" }`                  | `rewind_sdk::always!(cond, "id", "fmt", ...)`                      |
| Go       | `go get github.com/fzakaria/rewindvm/sdk/go`                                     | `rewind.Always(cond, "id", "fmt", ...)`, `rewind.Track(t)`         |
| Python   | `pip install "git+https://github.com/fzakaria/rewindvm#subdirectory=sdk/python"` | `rewind_sdk.always(cond, "id")`; the pytest plugin loads by itself |
| Java     | copy [`java/dev/rewindvm/sdk`](java/dev/rewindvm/sdk)                            | `Rewind.always(cond, "id")`, `@ExtendWith(RewindExtension.class)`  |
| Shell    | copy [`sh/rewind.sh`](sh/rewind.sh)                                              | `rewind_always id "message" test "$x" -eq 3`                       |

## The lines

A library for another language writes these, each in a single `write`:

```text
rewind-sdk 1 always pass <id>
rewind-sdk 1 always fail <id> <message>
rewind-sdk 1 sometimes hit <id>
rewind-sdk 1 sometimes miss <id>
rewind-sdk 1 unreachable <id> <message>
rewind-sdk 1 test-begin <name>
rewind-sdk 1 test-end pass|fail|skip <name>
```

A message and a test's name are the rest of the line. The device keeps a
write of up to 4096 bytes as one line.
