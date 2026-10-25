# A root filesystem for checks.determinism-sources: static busybox and a
# probe for each source of difference a guest might reach past the
# monitor, built with their symbols.
{ pkgs }:
pkgs.runCommand "rewind-determinism-root"
  {
    nativeBuildInputs = [ pkgs.pkgsStatic.stdenv.cc ];
    dontStrip = true;
  }
  ''
    mkdir -p $out/bin $out/src
    cp ${pkgs.pkgsStatic.busybox}/bin/busybox $out/bin/
    for tool in sh cat grep; do
      ln -s busybox $out/bin/$tool
    done
    for probe in counters xinuse results rdrand hidden; do
      cp ${./.}/$probe.c $out/src/
      $CC -static -O1 -g -mrdrnd -mavx2 -fdebug-prefix-map=$out/src=/src \
        -o $out/bin/$probe $out/src/$probe.c
    done
  ''
