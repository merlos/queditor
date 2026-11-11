# Releasing Queditor

Queditor publishes unsigned standalone archives through `.github/workflows/release.yml`.

## Release targets

- `x86_64-unknown-linux-musl`: static Linux x86-64 tarball.
- `x86_64-pc-windows-msvc`: Windows x86-64 zip.
- `x86_64-apple-darwin`: Intel macOS tarball.
- `aarch64-apple-darwin`: Apple Silicon macOS tarball.

Each native GitHub-hosted runner executes Rust tests, builds with `--release
--locked`, runs `queditor --version` and `queditor --check`, and packages the
binary with the license and user documentation. Linux additionally rejects a
binary with unexpected shared-library dependencies.

## Test without publishing

Run **Release binaries** from the GitHub Actions UI. A manual run builds all four
targets and exposes archives plus per-archive `.sha256` files as workflow artifacts.
It does not create a GitHub Release.

## Publish a release

1. Update `Cargo.toml` to the intended semantic version and refresh `Cargo.lock`.
2. Update release-facing documentation and run the required checks from `AGENTS.md`.
3. Commit and push the release changes.
4. Create and push a matching tag, for example:

   ```sh
   git tag -a v0.1.0 -m "Queditor v0.1.0"
   git push origin v0.1.0
   ```

The workflow refuses a tag that does not exactly equal `v` plus the version in
`Cargo.toml`. After every target succeeds, it creates a GitHub Release with all
archives, per-archive checksums, aggregate `SHA256SUMS`, and generated release notes.
If the tag's release already exists, the workflow uploads its artifacts there and
replaces any matching artifact names; otherwise, it creates the release. If any target
fails, no release is published.

Do not reuse or move a published version tag. Fix the issue, increment the version,
and publish a new tag.

## Signing status

These artifacts are currently unsigned. Windows code signing and Apple signing/
notarization require protected certificates, platform tooling, and repository
secrets and are intentionally deferred. When added, signing must occur before
archive checksums are generated.
