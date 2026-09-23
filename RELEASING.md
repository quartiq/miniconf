# Releasing

**Maintainer:** prepare and merge a release PR, then push the tags.
**Actions:** validate and publish the tagged packages.

Package versions advance independently. MQTT compatibility is defined by
`proto: 1`; incompatible wire changes require a new protocol version.

## 1. Prepare the release PR

Start a branch from current main. Choose versions and commit curated release
notes to the changelogs. Use a minor bump for breaking 0.x API changes.

**Rust:** on a clean working tree, substitute the crate and bump below.
Cargo-release updates versions, dependency requirements and changelog headings,
then commits. Omit `--execute` to preview any cargo-release command.

```sh
cargo release patch -p miniconf_mqtt --no-publish --no-tag --no-push --execute
```

Prepare dependencies before dependents. Review all resulting version changes.

**Python:** update `py/pyproject.toml` and `py/CHANGELOG.md`, then commit.

Push and open the PR:

```sh
git push -u origin HEAD
gh pr create
```

## 2. Merge and tag

Merge the PR, then update local main:

```sh
git switch main
git pull --ff-only
```

Confirm HEAD is the intended release commit, with no unmerged local commits,
and that its main CI run passed. Publishing workflows do not check CI history.

**Rust:** create and push the manifest-derived `<crate>-v<version>` tag:

```sh
cargo release tag -p miniconf_mqtt --execute
cargo release push -p miniconf_mqtt --execute
```

The push step also pushes the current branch. Do not run `cargo release publish`.

Release changed derive, core, then transport crates, **one tag at a time**.
Wait for each dependency to become available in the registry before proceeding.

**Python:** substitute the version from `py/pyproject.toml`:

```sh
git tag -a miniconf-mqtt-v<version> -m "Release miniconf-mqtt <version>"
git push origin miniconf-mqtt-v<version>
```

Only repository admins can create release tags; GitHub reports this as an
authorized ruleset bypass. Release tags cannot be moved or deleted.

## 3. Verify publication

A tag push starts Actions. Both publishers check the manifest version and main
ancestry; only publishing jobs receive registry credentials.

| Workflow | Automated work |
| --- | --- |
| Rust | Package against registry dependencies, then `cargo publish`. Tests run in ordinary CI. |
| Python | Build sdist and wheel, test the wheel against the Rust MQTT fixture, then upload those distributions. |

Check the publishing run and install the released package from its registry.

For an unpublished preview, use `cargo package -p <crate> --all-features` or
manually run **Release Python**.

## If publication fails

Inspect the registry first. If nothing was uploaded, fix configuration and rerun.
Source changes require a new version and tag. Verify published dependencies
before continuing.

For a partial PyPI upload, download the original `python-dist` workflow artifact,
compare existing file hashes, and have a registry owner upload only the missing
files. Reuse the original files; do not rebuild them or suppress upload errors.
