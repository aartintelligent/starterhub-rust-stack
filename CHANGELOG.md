# Changelog

## [1.1.1](https://github.com/aartintelligent/starterhub-rust-stack/compare/v1.1.0...v1.1.1) (2026-07-22)


### Bug Fixes

* **api:** close the non-JSON and slow-client gaps ([#46](https://github.com/aartintelligent/starterhub-rust-stack/issues/46)) ([762cefd](https://github.com/aartintelligent/starterhub-rust-stack/commit/762cefd201cb09414efc918a20af642585b2dc0c))
* **config:** make loading strict, lossless and hermetic ([#45](https://github.com/aartintelligent/starterhub-rust-stack/issues/45)) ([401ef04](https://github.com/aartintelligent/starterhub-rust-stack/commit/401ef04743d4df23b370308c33d30d1b5a13d27b))
* **cron:** bound job runs and drain them at shutdown ([#47](https://github.com/aartintelligent/starterhub-rust-stack/issues/47)) ([2a5e259](https://github.com/aartintelligent/starterhub-rust-stack/commit/2a5e25907608e839db10c6ab2bfef35d27226f88))
* make shutdown escalable and bounded ([#48](https://github.com/aartintelligent/starterhub-rust-stack/issues/48)) ([416b171](https://github.com/aartintelligent/starterhub-rust-stack/commit/416b1713ca94fe338c7a01d4413b8cc4660c86be))
* **migration:** lock concurrent boot-time runs ([#49](https://github.com/aartintelligent/starterhub-rust-stack/issues/49)) ([e6a96aa](https://github.com/aartintelligent/starterhub-rust-stack/commit/e6a96aada5bcb8c20e59781e4ba8621100ec541a))

## [1.1.0](https://github.com/aartintelligent/starterhub-rust-stack/compare/v1.0.0...v1.1.0) (2026-07-22)


### Features

* **cron:** log the hello job at info level ([#43](https://github.com/aartintelligent/starterhub-rust-stack/issues/43)) ([523c47d](https://github.com/aartintelligent/starterhub-rust-stack/commit/523c47dc6880fac850e5120ae39d6833b0911f7b))
* **cron:** skip overlapping job runs by default ([#41](https://github.com/aartintelligent/starterhub-rust-stack/issues/41)) ([62bcb65](https://github.com/aartintelligent/starterhub-rust-stack/commit/62bcb65a7a14f29b758e8ed24b9630968654212a))

## [1.0.0](https://github.com/aartintelligent/starterhub-rust-stack/compare/v0.7.0...v1.0.0) (2026-07-21)


### ⚠ BREAKING CHANGES

* **config:** deployments using alias spellings for `APP_ENVIRONMENT` must switch to the canonical lowercase values.

### Features

* **config:** restrict environment spellings ([#39](https://github.com/aartintelligent/starterhub-rust-stack/issues/39)) ([b4b2dca](https://github.com/aartintelligent/starterhub-rust-stack/commit/b4b2dca10e0958d8d4fd48ee477ee264b06fa4e7))

## [0.7.0](https://github.com/aartintelligent/starterhub-rust-stack/compare/v0.6.0...v0.7.0) (2026-07-21)


### Features

* review follow-ups with tests and ci hardening ([#26](https://github.com/aartintelligent/starterhub-rust-stack/issues/26)) ([f17bfea](https://github.com/aartintelligent/starterhub-rust-stack/commit/f17bfeac2baee253816f71b2d36adc90235ff6f3))

## [0.6.0](https://github.com/aartintelligent/starterhub-rust-stack/compare/v0.5.0...v0.6.0) (2026-07-21)


### Features

* request timeout, env-gated docs and dependency audit ([#25](https://github.com/aartintelligent/starterhub-rust-stack/issues/25)) ([c5d1cec](https://github.com/aartintelligent/starterhub-rust-stack/commit/c5d1cecfc5354542ce52eb51a1ccb9e6072ae20c))


### Bug Fixes

* **common:** safe pool defaults and url encoding ([#23](https://github.com/aartintelligent/starterhub-rust-stack/issues/23)) ([2bda066](https://github.com/aartintelligent/starterhub-rust-stack/commit/2bda066b6757247e384e75508bc720f4d432e28b))

## [0.5.0](https://github.com/aartintelligent/rust-service-starter/compare/v0.4.0...v0.5.0) (2026-07-21)


### Features

* rename the starter to starterhub-rust-stack ([#19](https://github.com/aartintelligent/rust-service-starter/issues/19)) ([9663649](https://github.com/aartintelligent/rust-service-starter/commit/9663649f77a87927d7bc776f81c1679fda232115))

## [0.4.0](https://github.com/aartintelligent/ipam/compare/v0.3.0...v0.4.0) (2026-07-21)


### Features

* rebrand the workspace as rust-service-starter ([#17](https://github.com/aartintelligent/ipam/issues/17)) ([dc0adee](https://github.com/aartintelligent/ipam/commit/dc0adee7fc50fd0a3e78c2f58fbe3d2648f6173a))

## [0.3.0](https://github.com/aartintelligent/ipam/compare/v0.2.0...v0.3.0) (2026-07-21)


### Features

* **common:** expose the application identity in the configuration ([#11](https://github.com/aartintelligent/ipam/issues/11)) ([cb779dd](https://github.com/aartintelligent/ipam/commit/cb779dda884effb277664c9333e3854f5ccccddd))

## [0.2.0](https://github.com/aartintelligent/ipam/compare/v0.1.0...v0.2.0) (2026-07-20)


### Features

* **api:** serve the openapi contract through swagger ui ([d4a261d](https://github.com/aartintelligent/ipam/commit/d4a261daf3b64e68e9b7f60e70c1cd9d6f456e31))
* bootstrap the ipam workspace ([c28b4e0](https://github.com/aartintelligent/ipam/commit/c28b4e00c0be1d4926745424a6acb82b270db163))
