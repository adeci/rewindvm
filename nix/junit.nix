# JUnit's console launcher with the Jupiter engine and API, which nixpkgs
# does not package: one jar that compiles and runs JUnit 5 suites.
{ pkgs }:
pkgs.fetchurl {
  url = "https://repo1.maven.org/maven2/org/junit/platform/junit-platform-console-standalone/1.11.4/junit-platform-console-standalone-1.11.4.jar";
  hash = "sha256-sBbvaxw0VNbXwsiM4IHavyiWmWhq9mItbk4uG1S0ovw=";
}
