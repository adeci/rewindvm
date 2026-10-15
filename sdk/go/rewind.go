// SPDX-License-Identifier: MIT

// Package rewind is the Rewind SDK for Go: assertions and test marks a
// program writes to /dev/rewind, which `rewind check` judges on every run
// it tries. Outside Rewind there is no /dev/rewind and every call does
// nothing, so the calls can stay in production code. sdk/README.md says
// what each one means.
//
//	rewind.Always(done == 100, "pool-drains", "done=%d", done)
//	rewind.Sometimes(len(queue) > 1, "work-queued")
//	rewind.Unreachable("submit-after-close", "")
//
// An id has no spaces. Each id writes a pass or a hit once, and a failure
// or a miss once, per process, since every write costs the VM an exit.
// In a test, rewind.Track(t) marks the test's start and result.
package rewind

import (
	"fmt"
	"os"
	"strings"
	"sync"
	"testing"
)

const (
	device = "/dev/rewind"
	prefix = "rewind-sdk 1"
	// The device keeps a write of up to 4096 bytes as one mark.
	lineMax = 4000
)

// Result is how a test ended.
type Result int

const (
	Pass Result = iota
	Fail
	Skip
)

var (
	openOnce sync.Once
	file     *os.File
	sentMu   sync.Mutex
	sent     = map[string]bool{}
)

// dev is the device, opened once; nil outside Rewind.
func dev() *os.File {
	openOnce.Do(func() {
		f, err := os.OpenFile(device, os.O_WRONLY, 0)
		if err == nil {
			file = f
		}
	})
	return file
}

// say writes words as one line, in one write.
func say(words string) {
	f := dev()
	if f == nil {
		return
	}
	line := prefix + " " + strings.ReplaceAll(words, "\n", " ")
	if len(line) > lineMax {
		line = line[:lineMax]
	}
	_, _ = f.Write([]byte(line + "\n"))
}

// sayOnce writes the line words gives the first time key is seen only.
func sayOnce(key string, words func() string) {
	if dev() == nil {
		return
	}
	sentMu.Lock()
	first := !sent[key]
	sent[key] = true
	sentMu.Unlock()
	if first {
		say(words())
	}
}

// with is words followed by the message format makes, if any.
func with(words, format string, args []any) string {
	if format == "" {
		return words
	}
	return words + " " + fmt.Sprintf(format, args...)
}

// Always says cond holds every time the program gets here. It returns
// cond.
func Always(cond bool, id string, format string, args ...any) bool {
	if cond {
		sayOnce("always pass "+id, func() string { return "always pass " + id })
	} else {
		sayOnce("always fail "+id, func() string { return with("always fail "+id, format, args) })
	}
	return cond
}

// Sometimes says some run should make cond true here. It returns cond.
func Sometimes(cond bool, id string) bool {
	words := "sometimes miss " + id
	if cond {
		words = "sometimes hit " + id
	}
	sayOnce(words, func() string { return words })
	return cond
}

// Unreachable says no run should get here.
func Unreachable(id string, format string, args ...any) {
	sayOnce("unreachable "+id, func() string { return with("unreachable "+id, format, args) })
}

// Point is a schedule point: an exit where `rewind check` can switch to
// another thread or hold this one up, with nothing written to the run's
// trace. Put one where a switch matters but the code makes no system call,
// such as between an atomic load and the store computed from it.
func Point() {
	if f := dev(); f != nil {
		_, _ = f.Write(nil)
	}
}

// TestBegin says a test named name, which may have spaces, starts.
func TestBegin(name string) {
	say("test-begin " + name)
}

// TestEnd says the test named name ended as result says.
func TestEnd(name string, result Result) {
	word := map[Result]string{Pass: "pass", Fail: "fail", Skip: "skip"}[result]
	say("test-end " + word + " " + name)
}

// Track marks t's start now and its result when it ends.
func Track(t testing.TB) {
	name := t.Name()
	TestBegin(name)
	t.Cleanup(func() {
		switch {
		case t.Skipped():
			TestEnd(name, Skip)
		case t.Failed():
			TestEnd(name, Fail)
		default:
			TestEnd(name, Pass)
		}
	})
}
