# SPDX-License-Identifier: MIT
"""The Rewind SDK for Python.

Assertions and test marks a program writes to /dev/rewind, which
`rewind check` judges on every run it tries. Outside Rewind there is no
/dev/rewind and every call does nothing, so the calls can stay in
production code. sdk/README.md says what each one means.

    import rewind_sdk as rewind

    rewind.always(value == cache.last_written("k"), "never-stale", f"got={value}")
    rewind.sometimes(len(waiting) > 1, "readers-overlapped")

An id has no spaces. Each id writes a pass or a hit once, and a failure
or a miss once, per process, since every write costs the VM an exit.
For pytest, the plugin rewind_sdk.pytest_plugin marks each test's start
and result.
"""

import os
import threading

DEVICE = "/dev/rewind"
PREFIX = "rewind-sdk 1"
# The device keeps a write of up to 4096 bytes as one mark.
LINE_MAX = 4000

PASS = "pass"
FAIL = "fail"
SKIP = "skip"

_lock = threading.Lock()
_sent = set()
_fd = None
_opened = False


def _device():
    """The device's descriptor, opened once; None outside Rewind."""
    global _fd, _opened
    with _lock:
        if not _opened:
            _opened = True
            try:
                _fd = os.open(DEVICE, os.O_WRONLY | os.O_CLOEXEC)
            except OSError:
                _fd = None
        return _fd


def _say(words):
    """Writes `words` as one line, in one write."""
    fd = _device()
    if fd is None:
        return
    line = f"{PREFIX} {words}".replace("\n", " ").encode()[:LINE_MAX] + b"\n"
    try:
        os.write(fd, line)
    except OSError:
        pass


def _say_once(key, words):
    """Writes the line `words` gives the first time `key` is seen only."""
    if _device() is None:
        return
    with _lock:
        if key in _sent:
            return
        _sent.add(key)
    _say(words())


def _with(words, message):
    return f"{words} {message}" if message else words


def always(cond, id, message=""):
    """`cond` holds every time the program gets here. Returns `cond`."""
    if cond:
        _say_once(f"always pass {id}", lambda: f"always pass {id}")
    else:
        _say_once(f"always fail {id}", lambda: _with(f"always fail {id}", message))
    return cond


def sometimes(cond, id):
    """Some run should make `cond` true here. Returns `cond`."""
    words = f"sometimes {'hit' if cond else 'miss'} {id}"
    _say_once(words, lambda: words)
    return cond


def unreachable(id, message=""):
    """No run should get here."""
    _say_once(f"unreachable {id}", lambda: _with(f"unreachable {id}", message))


def point():
    """A schedule point: an exit where `rewind check` can switch to another
    thread or hold this one up, with nothing written to the run's trace."""
    fd = _device()
    if fd is None:
        return
    try:
        os.write(fd, b"")
    except OSError:
        pass


def test_begin(name):
    """A test named `name`, which may have spaces, starts."""
    _say(f"test-begin {name}")


def test_end(name, result):
    """The test named `name` ended: PASS, FAIL or SKIP."""
    _say(f"test-end {result} {name}")
