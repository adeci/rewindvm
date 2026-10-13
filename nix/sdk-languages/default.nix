# A root filesystem for checks.sdk-languages: a program in each of C,
# Rust, Go, shell and Python that makes the Rewind SDK's calls, a pytest
# suite with the SDK's plugin, and a JUnit 5 suite with its extension, run
# one after another by /bin/sdk-demos. Each library is built from sdk/ at
# the repository's root, as a program that copies or depends on it would.
# The store paths they need are copied into the root's /nix/store.
{ pkgs }:
let
  sdk = ../../sdk;

  # JUnit's console launcher with the Jupiter engine and API, which
  # nixpkgs does not package.
  junit = pkgs.fetchurl {
    url = "https://repo1.maven.org/maven2/org/junit/platform/junit-platform-console-standalone/1.11.4/junit-platform-console-standalone-1.11.4.jar";
    hash = "sha256-sBbvaxw0VNbXwsiM4IHavyiWmWhq9mItbk4uG1S0ovw=";
  };

  c = pkgs.runCommandCC "sdk-languages-c" { } ''
    mkdir -p $out/bin
    $CC -Wall -Werror -I${sdk}/c ${./demo.c} -o $out/bin/demo-c
  '';

  rust =
    pkgs.runCommand "sdk-languages-rust"
      {
        nativeBuildInputs = [
          pkgs.rustc
          pkgs.stdenv.cc
        ];
      }
      ''
        mkdir -p $out/bin
        rustc --edition 2021 --crate-type rlib --crate-name rewind_sdk ${sdk}/rust/src/lib.rs -o librewind_sdk.rlib
        rustc --edition 2021 ${./demo.rs} --extern rewind_sdk=librewind_sdk.rlib -o $out/bin/demo-rust
      '';

  go =
    pkgs.runCommand "sdk-languages-go"
      {
        nativeBuildInputs = [ pkgs.go ];
      }
      ''
        mkdir -p $out/bin demo
        export HOME=$TMPDIR GOCACHE=$TMPDIR/cache GOFLAGS=-mod=mod GOPROXY=off CGO_ENABLED=0
        cp ${./demo.go} demo/main.go
        cd demo
        cat > go.mod <<EOF
        module demo

        go 1.21

        require github.com/fzakaria/rewindvm/sdk/go v0.0.0
        replace github.com/fzakaria/rewindvm/sdk/go => ${sdk}/go
        EOF
        go build -o $out/bin/demo-go .
      '';

  python = pkgs.python3.withPackages (ps: [ ps.pytest ]);

  java =
    pkgs.runCommand "sdk-languages-java"
      {
        nativeBuildInputs = [ pkgs.jdk_headless ];
      }
      ''
        mkdir -p $out/classes
        javac -Werror -d $out/classes -cp ${junit} \
          ${sdk}/java/dev/rewindvm/sdk/Rewind.java \
          ${sdk}/java/dev/rewindvm/sdk/RewindExtension.java \
          ${./DemoTest.java}
      '';

  # The tests fail on purpose, so their runners' exit statuses do not
  # end the script.
  demos = pkgs.writeShellScript "sdk-demos" ''
    ${c}/bin/demo-c
    ${rust}/bin/demo-rust
    ${go}/bin/demo-go
    ${pkgs.bash}/bin/sh ${./demo.sh} ${sdk}/sh/rewind.sh
    PYTHONPATH=${sdk}/python ${python}/bin/python3 ${./demo.py}
    cd ${./.}
    PYTHONPATH=${sdk}/python PYTHONDONTWRITEBYTECODE=1 \
      ${python}/bin/python3 -m pytest -q -p no:cacheprovider -p rewind_sdk.pytest_plugin test_demo.py || true
    JAVA_TOOL_OPTIONS='-Xmx256m -XX:+UseSerialGC' ${pkgs.jdk_headless}/bin/java -jar ${junit} \
      execute --disable-banner --details=none --class-path ${java}/classes --select-class DemoTest || true
  '';

  closure = pkgs.closureInfo { rootPaths = [ demos ]; };
in
{
  inherit demos;
  root = pkgs.runCommand "rewind-sdk-languages-root" { } ''
    mkdir -p $out/bin $out/nix/store
    while read -r path; do
      cp -a "$path" $out/nix/store/
    done < ${closure}/store-paths
    ln -s ${demos} $out/bin/sdk-demos
  '';
}
