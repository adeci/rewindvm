# hash-order: a test that expects a HashSet's order, which Rust draws from
# the system's randomness.
RUSTC ?= rustc
PREFIX ?= /usr/local

all: tags-test

tags-test: tags.rs
	$(RUSTC) --edition 2021 --test -O tags.rs -o tags-test

check: tags-test
	./tags-test

install: tags-test
	mkdir -p $(PREFIX)/bin
	cp tags-test $(PREFIX)/bin/

clean:
	rm -f tags-test

.PHONY: all check install clean
