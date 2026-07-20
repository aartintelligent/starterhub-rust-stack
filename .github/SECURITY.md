# Security policy

## Supported versions

Only the latest release receives security fixes.

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Use GitHub's
private vulnerability reporting instead: *Security* tab → *Report a
vulnerability*. Reports are acknowledged within a few days and fixed
versions are published through the regular release pipeline.

Dependency advisories are monitored automatically: Dependabot keeps the
tree up to date and the `audit` workflow checks the lockfile against the
[RustSec](https://rustsec.org) database weekly.
