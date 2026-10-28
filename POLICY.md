# Listing policy

This repository publishes an index of extensions. This page says what may be
listed, what an extension must say about itself, and what listing does and does
not mean.

## What may be listed

An extension may be listed when:

- it builds from source the maintainer can read and build;
- its author is named, and holds the rights to publish it;
- it carries a licence, and that licence allows distribution from here;
- its manifest declares its network grants, and its `Cargo.toml` declares the
  disclosures a person should read before installing it (`[package.metadata.vrc-we] disclosures`);
- where its users must accept terms of the author's or a service's, it links to
  them (`terms` in the manifest), so the client can show them before an install.

An extension that needs its own service, its own repository or a paid account
may be listed. The listing says what it needs; the service is the author's.

## What listing means

The maintainer reads an extension before it is listed. That check covers what
this policy names, and nothing more. Listing is not a warranty that an extension
is safe, lawful where you are, or fit for any purpose, and it is not an
endorsement by the maintainer or by the client's author.

Each extension is a separate product, manufactured by its author. Where the
maintainer wrote it, the maintainer is its author. Its defects, its handling of
your data and the terms you accept are the author's.

## What is removed

An extension is removed from the index when:

- it does something its manifest, its disclosures or this policy does not
  declare;
- it serves code that its listed source does not build;
- its author asks people to run it outside these terms;
- its author asks for its removal, or the maintainer cannot keep listing it.

Removal is from the index alone. What was installed stays installed: the client
does not remove an extension because a repository stopped listing it.

## Reports

Report a problem with an extension through this repository's issues. The
maintainer reads them and acts on what this policy names. Nothing here monitors
what an installed extension does at runtime; the client's sandbox and the grants
a person approved are the boundary.

## Releases and store listings

The client is what ships. Extensions are fetched at runtime from a repository a
person chose, so no extension is part of the build.

That is also the answer to a store's content survey, and it is a checklist item
for every release: if an extension is ever shipped inside the build or the store
package, what the build contains changes and the survey answer changes with it.
Extensions listed here are not shipped.
