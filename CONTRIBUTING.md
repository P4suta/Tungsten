# Contributing

Tungsten is a personal Markdown note service for trusted devices inside a private tailnet.
Read the [features](docs/features.md), [storage decisions](docs/storage-evaluation.md), and [security boundaries](docs/security.md) before changing behavior.

## Development

Install the versions pinned in `mise.toml` and prepare the project.

```sh
mise trust
mise install
mise x -- just setup
```

Run `mise x -- just check` before submitting changes.
The command checks Rust, TypeScript, browser workflows, documentation, dependencies, and secrets.
Use `mise x -- just --list` to find the maintained commands.
Full proofs, fuzzing, mutation testing, and resource-intensive campaigns belong to candidate verification.

## Changes

Open an issue describing the problem, intended behavior, scope, and validation.
Keep pull requests focused and use an English Conventional Commits title.
Include the reason, resulting behavior, and actual validation results.
Record consequential architecture choices in `docs/adr`.
Before the first release, storage formats can change without migration support.

Contributions use `MIT OR Apache-2.0` unless the contributor explicitly states other terms.
Report vulnerabilities through the [security policy](SECURITY.md).
