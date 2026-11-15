# AGENTS.md: anolis-provider-template-rust

The template every Rust provider starts from. Changes here reach every provider
made from it afterwards, so keep it small: one synthetic device, the SDK, and
the build and release a provider needs.

## Build / test

- `just check` (rustfmt, clippy with `-D warnings`) and `just test` (unit tests, then the conformance harness against
  the debug build). CI also builds the static musl binary on x86_64 and arm64 and runs the harness against it.
- The required CI status check is the **`ok`** job; never merge red.
- `scripts/conformance.py` requires the harness to pass with no skips beyond `EXPECTED_SKIPS`, which is empty.

## Rules

- **Self-contained.** CI and release use only public actions and public anolishq releases: no reusable anolishq
  workflows, actions or secrets. A provider made from the template must build and release in any account. That is
  why `ok` checks the job results inline rather than through `anolishq/.github`'s action.
- **`scripts/rename.sh` must keep working.** The `rename` CI job renames the template twice and runs the checks; a
  new file that names the template has to be covered by the script.
- **The heater matches the C++ and Python templates.** Keep the three the same device.
- **The release binary is static** (musl). CI fails if it links a shared library, so it loads on any Linux host
  whatever its glibc.
- The SDK is pinned by release tag in `Cargo.toml`; `Cargo.lock` records the commit the tag resolved to.
- The template is never tagged or released itself.
- An outside instance generated from this template proves the path from another account; if the build or release
  path changes, repeat that proof.
