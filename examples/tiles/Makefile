# tiles: a library's pkg-config file, written by a Python script whose
# Requires line comes from a set.
PYTHON ?= python3
PREFIX ?= /usr/local

all: tiles.pc

tiles.pc: gen-pc.py
	$(PYTHON) gen-pc.py $(PREFIX) > $@

install: tiles.pc
	install -D -m644 tiles.pc $(PREFIX)/lib/pkgconfig/tiles.pc

clean:
	rm -f tiles.pc

.PHONY: all install clean
