# The examples the tutorials use. mylib is a small C thread pool whose
# shutdown test fails only under some thread interleavings: the Nix
# tutorial builds it with `rewind check` and scrubs the failure.
# philosophers is the dining philosophers, whose check deadlocks under
# some interleavings and is ended by a timeout. bank loses a deposit to
# two threads racing on one balance, config-reload has a reader process
# see a config half rewritten, and waiter's parent sleeps through the
# signal that should wake it; each fails only under some interleavings.
# hash-order's test expects a HashSet's order, which Rust draws from the
# system's randomness, so it fails only under some seeds.
{ pkgs }:
{
  mylib = pkgs.stdenv.mkDerivation {
    pname = "mylib";
    version = "0.3.0";
    src = ../examples/mylib;
    makeFlags = [ "PREFIX=$(out)" ];
    doCheck = true;
    meta.description = "A thread pool with a shutdown race, for the Rewind VM tutorials";
  };

  philosophers = pkgs.stdenv.mkDerivation {
    pname = "philosophers";
    version = "0.1.0";
    src = ../examples/philosophers;
    makeFlags = [ "PREFIX=$(out)" ];
    doCheck = true;
    meta.description = "The dining philosophers, deadlocking under some interleavings";
  };

  bank = pkgs.stdenv.mkDerivation {
    pname = "bank";
    version = "0.1.0";
    src = ../examples/bank;
    makeFlags = [ "PREFIX=$(out)" ];
    doCheck = true;
    meta.description = "Two tellers and one account, losing a deposit under some interleavings";
  };

  config-reload = pkgs.stdenv.mkDerivation {
    pname = "config-reload";
    version = "0.1.0";
    src = ../examples/config-reload;
    makeFlags = [ "PREFIX=$(out)" ];
    doCheck = true;
    meta.description = "A config rewritten in place, read half written under some interleavings";
  };

  hash-order = pkgs.stdenv.mkDerivation {
    pname = "hash-order";
    version = "0.1.0";
    src = ../examples/hash-order;
    nativeBuildInputs = [ pkgs.rustc ];
    makeFlags = [ "PREFIX=$(out)" ];
    doCheck = true;
    meta.description = "A test that expects a HashSet's order, failing under some seeds";
  };

  waiter = pkgs.stdenv.mkDerivation {
    pname = "waiter";
    version = "0.1.0";
    src = ../examples/waiter;
    makeFlags = [ "PREFIX=$(out)" ];
    doCheck = true;
    meta.description = "A parent waiting with pause, sleeping through its wakeup under some interleavings";
  };
}
