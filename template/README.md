# template

A starting point for an extension. One namespace, one page, one action, one condition, one stored value, one writable field, and tests. Copy the directory, rename it, build.

```bash
cp -r template extensions/mine
```

Rename in this order: `Cargo.toml`'s `name`, the `[workspace] members` entry, `Manifest.name`, `Manifest.author` (`"your name"` is a placeholder), the namespace `TEMPLATE`, and `PAGE`. Then build and test with the new crate name:

```bash
cargo wasm -p mine
cargo test -p mine
```

Publishing is a choice. Add the metadata block to the crate's `Cargo.toml` and the index tool picks it up. Without it, the crate builds and tests like any other and never enters a release.

```toml
[package.metadata.vrc-we]
index = true
```

## What it shows

- A `Manifest` with the smallest useful surface: one namespace, two fields, one method, one action, one condition, one page, no network and no secrets.
- `Extension::handle` as a match over `Event`. `Start` loads what it kept, `Call` answers, `Ui` reacts to its page.
- `Command::Field` publishes a value a rule can read. `Command::Signal` fires one. `Command::Store` keeps state across restarts. `Command::Page` draws the page.
- A `Toggle` whose value comes back as `UiEvent::Toggled`, next to a `Button` that only sends. Both directions of the view protocol are visible.
- Tests that drive the module as events and assert the commands. No client, no wasm, no runtime.

## Terms

An extension may name terms the person accepts before installing it. Declare them in the manifest, either as a
page or as text:

```rust
terms: Some(Terms {
    label: "My extension terms".into(),
    url: "https://example.org/terms".into(),
    ..Terms::default()
}),
```

Choose the URL, then keep it. Point at your own terms page when you publish one; otherwise point at the terms of
the service your extension calls, which is what the person is really agreeing to. One stable address, no query
string that changes between releases, and https only: the client refuses anything else.

Inline text, `inline`, travels inside the module and needs no network round trip. It is capped at 16 KiB, and it
freezes with the release, so a legal wording change costs a new version. A URL costs nothing to fix, which is why
the template shows that form.

The client shows the terms before the grants, records the hash of what it showed, and shows them again when the
hash changes, so the text a person accepted is the text they can prove they accepted.
