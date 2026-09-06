# FTS-Reaper: `nix run .#fts-reaper` — REAPER + SWS + ReaPack + this repo's
# own extension cdylib, pre-wired into a config dir. No manual setup, no
# separate `fts-installer reaper` step.
#
# Built from the vendored reaper-flake recipes (nix/vendor/reaper-flake,
# subtree-imported — see wrapper/reaper/pkgs/{reaper,sws,reapack}.nix) plus
# the `fts-extensions` crane package (this repo's own REAPER extension,
# apps/extensions/reaper-fts-extensions).
#
# The 17-plugin CLAP/VST3 suite that used to be injected alongside SWS/
# ReaPack here moved out with the August 2026 split — it lives in the
# `signal` repo now, which reaches REAPER as CLAP plugins rather than
# through this extension. This repo only ever contributes the one
# extension .so.
#
# Plugin injection follows the exact idiom reaper-flake already used for
# SWS/ReaPack: idempotent launch-time symlinks into $REAPER_CONFIG/
# UserPlugins — see wrapper/reaper/pkgs/dmg.nix's fts-reaper-launcher.
{ ... }:
{
  perSystem = { pkgs, lib, config, ... }:
    let
      vendor = ../../vendor/reaper-flake;

      reaper = pkgs.callPackage (vendor + "/wrapper/reaper/pkgs/reaper.nix") {
        jackLibrary = pkgs.pipewire.jack;
        libxml2 = pkgs.libxml2_13; # .so.2 — matches nixpkgs' libSwell build
      };
      sws = pkgs.callPackage (vendor + "/wrapper/reaper/pkgs/sws.nix") { };
      reapack = pkgs.callPackage (vendor + "/wrapper/reaper/pkgs/reapack.nix") { };

      # This repo's own REAPER extension cdylib, built via crane so it
      # shares the vendored-deps cache with every other deployable
      # package here (offline sandbox build). `daw-reaper`/`reaper-*`
      # need pkg-config + mold same as fts-extensions-actions' other
      # consumers, hence commonArgs rather than a bare buildPackage.
      fts-extensions-so = config.fts.craneLib.buildPackage (config.fts.commonArgs // {
        pname = "fts-extensions-so";
        version = "0.1.0";
        cargoArtifacts = null;
        cargoExtraArgs = "--package fts-extensions --release";
        buildInputs = config.fts.buildInputs;
        doNotPostBuildInstallCargoBinaries = true;
        doCheck = false;
        # `fts-extensions` is a cdylib (crate-type = ["cdylib", "rlib"]) —
        # crane's default `cargo install` step only knows binaries, so
        # copy the built shared object out by hand. REAPER's loader
        # expects the plugin filename without the `lib` prefix.
        installPhaseCommand = ''
          mkdir -p $out
          cp target/release/libreaper_fts_extensions.so $out/reaper_fts_extensions.so
        '';
      });

      reaperConfig = "\${FTS_REAPER_CONFIG:-$HOME/fasttrackstudio}";

      # FTS default reaper.ini — general prefs only (audio driver mode,
      # undo memory, the reaper_fts_extensions dock layout, SWS loudness
      # targets, toolbar geometry). No machine-specific window positions
      # or audio/MIDI device selection — those stay on REAPER's own
      # auto-detect. Sourced from the actual production rig's config
      # (~/fasttrackstudio/reaper.ini), hardware-specific bits stripped.
      reaperIniTemplate = vendor + "/assets/reaper.ini.template";

      # This repo's versioned REAPER configuration — keybindings,
      # toolbars, mouse modifiers, FX tags/folders, screensets, the
      # active theme, and the ReaPack manifest. See
      # nix/reaper-config/README.md for what is and is not in here.
      #
      # ~4 MB, because ReaPack's ~994 downloaded scripts are NOT
      # versioned: `ReaPack/registry.db` is the manifest they are
      # restored from. First launch on a new machine therefore needs one
      # ReaPack "synchronise packages" to fetch them.
      ftsReaperConfig = ../../reaper-config;

      fts-reaper = pkgs.writeShellApplication {
        name = "fts-reaper";
        text = ''
          CONFIG_DIR="${reaperConfig}"
          mkdir -p "$CONFIG_DIR/UserPlugins" "$CONFIG_DIR/Scripts"

          # Never clobber a configured rig — only seed the FTS defaults
          # the first time this config dir is used.
          # install -m: the nix store source is read-only; REAPER needs to
          # write this file back on exit.
          [ -f "$CONFIG_DIR/reaper.ini" ] || install -m 644 "${reaperIniTemplate}" "$CONFIG_DIR/reaper.ini"

          # The versioned configuration. Copied (not symlinked) and made
          # writable: REAPER rewrites these files as you work, and a
          # symlink into the read-only nix store would make every
          # toolbar edit fail.
          #
          # Absolute paths inside reaper.ini were tokenised on export —
          # the active theme's path among them — so they are expanded to
          # this machine's config dir here. Without that, a config
          # exported on one machine points at a directory that does not
          # exist on the next.
          for f in "${ftsReaperConfig}"/*.ini "${ftsReaperConfig}"/*.db; do
            [ -e "$f" ] || continue
            install -m 644 "$f" "$CONFIG_DIR/$(basename "$f")"
          done
          for d in ColorThemes MenuSets TrackTemplates ProjectTemplates Configurations ReaPack; do
            if [ -d "${ftsReaperConfig}/$d" ]; then
              mkdir -p "$CONFIG_DIR/$d"
              cp -RL --no-preserve=mode "${ftsReaperConfig}/$d"/. "$CONFIG_DIR/$d/"
            fi
          done
          # Our own scripts and JSFX, alongside whatever ReaPack manages.
          for d in Scripts Effects; do
            if [ -d "${ftsReaperConfig}/$d" ]; then
              mkdir -p "$CONFIG_DIR/$d"
              cp -RL --no-preserve=mode "${ftsReaperConfig}/$d"/. "$CONFIG_DIR/$d/"
            fi
          done

          # Reuse a licence this machine already has.
          #
          # The key is personal and deliberately NOT versioned — putting
          # it in a public repo would be publishing it. But a machine
          # that already runs REAPER has one lying around, and making
          # someone re-enter it just because they launched a different
          # config dir is pointless friction. So: look in the usual
          # places, copy the first hit, and never overwrite one that is
          # already here.
          if [ ! -f "$CONFIG_DIR/reaper-license.rk" ]; then
            for candidate in \
              "''${FTS_REAPER_LICENSE:-}" \
              "$HOME/.config/REAPER/reaper-license.rk" \
              "$HOME/.config/reaper/reaper-license.rk" \
              "$HOME/fts-dev/reaper-license.rk" \
              "$HOME/.reaper/reaper-license.rk" \
              "$HOME/Library/Application Support/REAPER/reaper-license.rk"; do
              if [ -n "$candidate" ] && [ -f "$candidate" ]; then
                install -m 600 "$candidate" "$CONFIG_DIR/reaper-license.rk"
                echo "fts-reaper: reusing licence from $candidate" >&2
                break
              fi
            done
          fi

          # $REAPER_RESOURCES → this config dir.
          if grep -q 'REAPER_RESOURCES' "$CONFIG_DIR/reaper.ini" 2>/dev/null; then
            sed -i "s|\$REAPER_RESOURCES|$CONFIG_DIR|g" "$CONFIG_DIR/reaper.ini"
          fi

          ln -sf "${sws}"/UserPlugins/* "$CONFIG_DIR/UserPlugins/" 2>/dev/null || true
          ln -sf "${reapack}"/UserPlugins/* "$CONFIG_DIR/UserPlugins/" 2>/dev/null || true
          ln -sf "${fts-extensions-so}/reaper_fts_extensions.so" "$CONFIG_DIR/UserPlugins/reaper_fts_extensions.so"

          exec "${reaper}/bin/reaper" -cfgfile "$CONFIG_DIR/reaper.ini" -newinst "$@"
        '';
      };
    in
    {
      packages = { inherit reaper sws reapack fts-extensions-so fts-reaper; };
    };
}
