// The Go SDK's calls, for checks.sdk-languages.
package main

import rewind "github.com/fzakaria/rewindvm/sdk/go"

func main() {
	rewind.TestBegin("go demo")
	rewind.Always(true, "go-holds", "")
	for i := 0; i < 2; i++ {
		rewind.Always(false, "go-fails", "%s said so", "go")
	}
	rewind.Sometimes(false, "go-sometimes")
	rewind.Unreachable("go-unreachable", "reached")
	rewind.TestEnd("go demo", rewind.Fail)
}
