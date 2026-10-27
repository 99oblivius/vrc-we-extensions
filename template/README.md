# template

A starting point for an extension: one namespace, one page, one action, one
condition, its own stored value, and its tests. Copy the directory, rename the
package and the namespace, and build.

```bash
cp -r template extensions/mine
# Cargo.toml: name = "mine"; lib.rs: Manifest.name, Namespace.name, PAGE
cargo wasm -p mine
cargo test -p mine
```

## What it shows

- `Manifest` with the smallest useful surface: a namespace with one writable
  field, one method, one action, one condition, one page, no network and no
  secrets.
- `Extension::handle` as a match over `Event`: `Start` loads what it kept,
  `Call` answers, `Ui` reacts to its page, `Timer` ticks.
- `Command::Field` to publish a value a rule can read, `Command::Signal` to fire
  one, `Command::Store` to keep state across restarts, `Command::Page` to draw
  its page.
- Tests that drive it as events and assert the commands, with no client and no
  wasm — the module is a pure state machine.

Replace this file's contents as you go; the README is yours to rewrite.
