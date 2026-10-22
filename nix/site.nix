# `nix build .#site`: the marketing site exactly as the pages workflow
# deploys it and `nix run .#serve` previews it. Downloads are GitHub release
# assets, built by the release workflow, so the site is only the site.
#
# The site directory alone rather than a subpath of the flake source, so a
# change to the engine or the app does not rebuild the site.
#
# The viewer is the desktop app built for the browser (nix/web.nix): its
# wasm and glue go in under viewer/pkg and its fonts under viewer/fonts,
# where site/viewer/app.js loads them.
{ pkgs, web }:
pkgs.runCommand "rewind-site" { } ''
  mkdir -p $out
  cp -r ${../site}/. $out/
  chmod -R u+w $out/viewer
  cp -r ${web}/pkg ${web}/fonts $out/viewer/

  # The release the front page names, from VERSION.
  chmod u+w $out/index.html
  substituteInPlace $out/index.html --replace-fail @VERSION@ ${pkgs.lib.fileContents ../VERSION}
''
