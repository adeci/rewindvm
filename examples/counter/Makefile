# counter: two threads and one count, and a check that no hit is lost.
CC ?= cc
CFLAGS ?= -O1 -g -Wall
LDLIBS = -lpthread
PREFIX ?= /usr/local

all: counter

counter: counter.c rewind.h
	$(CC) $(CFLAGS) -o $@ counter.c $(LDLIBS)

check: counter
	./counter

install: counter
	mkdir -p $(PREFIX)/bin
	cp counter $(PREFIX)/bin/

clean:
	rm -f counter

.PHONY: all check install clean
