# `nix build .#web`: the desktop app built for the browser, the site's
# viewer (crates/rewind-app/src/bin/rewind-web.rs): its wasm, shrunk with
# wasm-opt, the JS glue wasm-bindgen writes for it, and the fonts the app
# is set in, which the page fetches at start. nix/site.nix puts the wasm
# and glue under viewer/pkg and the fonts under viewer/fonts.
{
  pkgs,
  rust,
  releaseDate,
}:
let
  # The app's dependencies compiled for wasm32 with the web profile, once
  # per Cargo.lock and set of manifests.
  cargoArtifacts = rust.craneLib.buildDepsOnly (rust.web // { pname = "rewind-web"; });

  wasm = rust.craneLib.buildPackage (
    rust.web
    // {
      pname = "rewind-web";
      inherit cargoArtifacts;

      # The day the commit was made, which build.rs reads; given to this
      # build alone, as nix/app.nix does.
      REWIND_RELEASE_DATE = releaseDate;

      installPhaseCommand = ''
        mkdir -p $out
        cp target/wasm32-unknown-unknown/web/rewind-web.wasm $out/
      '';
    }
  );

  # The fonts' files, as the desktop app's wrapper adds them (nix/app.nix):
  # the web platform needs them as files, and starts with none.
  plex = "${pkgs.ibm-plex}/share/fonts/opentype";
  jetbrains = "${pkgs.jetbrains-mono}/share/fonts/truetype";
in
pkgs.runCommand "rewind-web-${wasm.version}"
  {
    # The command must be the version crates/rewind-app pins.
    nativeBuildInputs = [
      pkgs.wasm-bindgen-cli_0_2_127
      pkgs.binaryen
    ];
  }
  ''
    mkdir -p $out/pkg $out/fonts
    wasm-bindgen --target web --no-typescript --out-dir $out/pkg ${wasm}/rewind-web.wasm
    wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
      --enable-mutable-globals --enable-reference-types --enable-multivalue \
      $out/pkg/rewind-web_bg.wasm -o $out/pkg/rewind-web_bg.wasm
    cp ${plex}/IBMPlexSans-{Regular,Italic,SemiBold,Bold}.otf $out/fonts/
    cp ${jetbrains}/JetBrainsMono-{Regular,Italic,Bold}.ttf $out/fonts/
  ''
