# Releasing

**The maintainer prepares the version and pushes a release tag. Actions builds,
checks, and publishes the package.** Merging a PR does not publish anything.

Package versions advance independently. MQTT compatibility is defined by
`proto: 1`; incompatible wire changes require a new protocol version.

## Each release: maintainer

1. **Prepare a PR.** Choose versions and update dependency requirements and
   changelogs. Use a minor bump for breaking 0.x API changes. Edit the Python
   version in `py/pyproject.toml`.
2. **Merge and check CI.** Confirm CI passed on the main commit being released.
   The release workflows do not check historical CI results.
3. **Tag and push.** Create an annotated tag on that commit using the table below.
   Push one tag at a time. Release changed derive, core, then transport crates;
   wait for each dependency to become available in the registry before proceeding.
4. **Verify the result.** Check the publishing run and install from the registry.

| Package | Release tag |
| --- | --- |
| Python `miniconf-mqtt` | `miniconf-mqtt-v<version>` |
| Rust crate | `<crate>-v<version>` |

For Rust preparation, cargo-release can update versions and changelog headings.
Review each preview before executing it; substitute the package and bump needed:

```sh
cargo release version patch -p miniconf
cargo release version patch -p miniconf --execute
cargo release replace -p miniconf
cargo release replace -p miniconf --execute
```

Use only these preparation steps; Actions handles publication after the tag push.

## After the tag push: Actions

Both workflows require the tag to match the manifest version and its commit to
belong to main. Only the publishing job receives registry credentials.

| Workflow | Automated work |
| --- | --- |
| Rust | Verify the package against registry dependencies, then run normal `cargo publish`. The Rust test suite runs in ordinary CI. |
| Python | Build the sdist and a wheel from it, install and test that wheel against the Rust MQTT fixture, then publish those same distribution files. |

For a preview, run `cargo package -p <crate> --all-features` locally, or manually
run the **Release Python** workflow. Neither preview publishes.

## Failed publication: maintainer

Inspect the registry before retrying. If nothing was uploaded, fix configuration
failures and rerun. Source changes require a new version and tag, never a moved tag.
Verify an already published crate before continuing with dependent releases.

For a partial PyPI upload, download the original `python-dist` workflow artifact,
compare existing file hashes, and have a registry owner upload only the missing
files. Reuse the original files; do not rebuild them or suppress upload errors.
