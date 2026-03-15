# Changelog

All notable changes to a2a-rust are documented in the project's [CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

## Versioning

a2a-rust follows [Semantic Versioning](https://semver.org/):

- **Major** (1.0.0) — Breaking API changes
- **Minor** (0.2.0) — New features, backward compatible
- **Patch** (0.2.1) — Bug fixes, backward compatible

All four workspace crates share the same version number and are released together.

## Release Process

Releases are triggered by pushing a version tag:

```bash
git tag v0.2.0
git push origin v0.2.0
```

The [release workflow](https://github.com/tomtom215/a2a-rust/blob/main/.github/workflows/release.yml) automatically:

1. Validates all crate versions match the tag
2. Runs the full CI suite
3. Publishes crates to crates.io in dependency order
4. Creates a GitHub release with notes

### Publish Order

```
a2a-types → a2a-client + a2a-server → a2a-sdk
```

This ensures each crate's dependencies are available before it publishes.

## Current Version

See the [CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md) for the latest release notes and version history.
