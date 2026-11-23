# Release tooling: package.sh and CI

`scripts/package.sh` labels the tarball from `Cargo.toml` alone, so a dirty tree or an untagged
HEAD is packaged as a release, and the local artifact is the one uploaded (real Y48). The CI
`package` job runs only on tags, so a break in the script or the Ubuntu 22.04 build shows after
the tag is pushed (real Y49).

- Where: `scripts/package.sh:10,28-31`, `.github/workflows/ci.yml:26-27`
- Done when: the script exits non-zero on a dirty tree or a missing matching tag; pushes to
  `main` run the script or a `--release --locked` build on ubuntu-22.04.
