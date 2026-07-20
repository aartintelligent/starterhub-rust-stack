<!--
  The PR title must be a Conventional Commit (it becomes the commit on
  main via squash merge): <type>(<scope>): <description>
  See CONTRIBUTING.md for the full ruleset.
-->

## What

<!-- What does this PR change, from the point of view of the service? -->

## Why

<!-- Link the issue if one exists, otherwise explain the motivation. -->

## Checklist

- [ ] `just ci` passes locally (fmt-check, clippy `-D warnings`, tests).
- [ ] The PR title follows [Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/).
- [ ] Every new item carries rustdoc; comments explain *why*, not *what*.
- [ ] New endpoints are annotated with `#[utoipa::path]` and registered in `ApiDoc`.
- [ ] No shipped migration was edited — schema changes are new files in `migration/src/source/`.
- [ ] Dependencies were added to `[workspace.dependencies]` only, features at the consuming crate.
