# Contributing

Pull requests are welcome. Open an issue first for anything larger than a fix, so the change can be agreed before you write it.

## Contribution license

The SDK in this repository is offered under the [Apache License 2.0](sdk/LICENSE); the template and the extensions are offered under the [MIT License](LICENSE).

By submitting a contribution (a pull request, patch, or any other material) to this repository, you confirm that you wrote it, or otherwise have the right to submit it under the terms of the licence of the part you changed, and that you keep the copyright in your contribution.

## Checks

Every pull request must pass:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

A new extension is a crate in `extensions/` that declares itself for the index (`[package.metadata.vrc-we] index = true`, and `disclosures = [ … ]` for what it can do that its grants do not say), so `cargo run -p index` finds it from cargo's own metadata rather than a list someone keeps by hand.

The client hosts these modules; its own checks stay the boundary, so an extension is written to the SDK's contract and not around it. A change to that contract — an event, a command, a variant or a field a module built earlier would not read — bumps `BOUNDARY_VERSION`. New fields carry `#[serde(default)]` instead.

An extension's own tests run in this workspace; a change to the camera or the template keeps `cargo test --workspace` green.

## Inclusion

An extension reaches the index by passing the rules in [POLICY.md](POLICY.md):
it builds from source, names its author and licence, declares its disclosures
and its grants, and links the terms its users accept.

## Releases

A tag names one version for the whole workspace, and the release checks every
listed extension against it. Changing one extension's version bumps the
workspace and every module with it, so the set moves together.

## Commit messages

`Area: what changed (why)`, several items joined with ` + `.
