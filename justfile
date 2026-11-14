# anolis-provider-template-rust dev commands. `check` + `test` are what CI's
# check and test jobs run; python3 is needed for the conformance harness.

harness_wheel := "https://github.com/anolishq/anolis-protocol/releases/download/v1.10.0/anolis_protocol-1.10.0-py3-none-any.whl"

# List available recipes
default:
    @just --list

# Install the toolchain components and the conformance harness
setup:
    rustup component add rustfmt clippy
    test -x .harness/bin/anolis-adpp-conformance || (python3 -m venv .harness && .harness/bin/pip install -q "anolis-protocol[conformance] @ {{harness_wheel}}")

# Format the code
fmt:
    cargo fmt

# CI-equivalent static checks: format, lint
check:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings

# Unit tests, and the conformance harness against the provider
test: setup
    cargo test --locked
    cargo build --locked
    python3 scripts/conformance.py target/debug/anolis-provider-template-rust .harness/bin/anolis-adpp-conformance

# Build the static release binary for this host (needs the musl target: rustup target add <arch>-unknown-linux-musl)
release:
    cargo build --locked --release --target "$(uname -m)-unknown-linux-musl"

# Rename the provider after "Use this template": just rename <kind> (e.g. bme280-lab)
rename kind:
    scripts/rename.sh {{kind}}
