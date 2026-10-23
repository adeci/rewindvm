# plugins: an index of the plugins a package ships, written by a Python
# script in the order the plugins directory lists them.
PYTHON ?= python3
PREFIX ?= /usr/local

all: plugins.txt

plugins.txt: gen-index.py plugins/*.ini
	$(PYTHON) gen-index.py plugins > $@

install: plugins.txt
	install -D -m644 plugins.txt $(PREFIX)/share/plugins/plugins.txt
	install -m644 plugins/*.ini $(PREFIX)/share/plugins/

clean:
	rm -f plugins.txt

.PHONY: all install clean
