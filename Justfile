# keyflow dev recipes
#
# Run from the repo root inside the Nix dev shell:
#   nix develop -c just <recipe>
#
# Most recipes delegate to `cargo xtask` (the shared fts-repo battery);
# docs/web recipes wrap the scripts they document.

# Default: type-check the workspace.
default: check

# Type-check workspace (all targets).
check:
    cargo xtask check

# nextest with the default profile.
test:
    cargo xtask test

# Full gate: fmt --check + clippy -D warnings + check + nextest ci profile.
ci:
    cargo xtask ci

# Regenerate CHANGELOG.md from conventional commits.
changelog:
    git cliff -o CHANGELOG.md

# Docsite dev loop — kf-block watcher + ddc live reload on :8080.
docs-serve:
    ./docs/serve.sh

# Build the docsite to docs/output.
docs-build:
    ./docs/build.sh

# Build + deploy the docsite to fly.io (app: keyflow-docs).
docs-deploy:
    ./docs/deploy.sh

# Web editor with live reload.
web-editor:
    dx serve --package web-editor --platform web
