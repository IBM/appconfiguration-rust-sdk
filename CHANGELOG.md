# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-rc.1](https://github.com/IBM/appconfiguration-rust-sdk/compare/v0.1.0-rc.0...v0.1.0-rc.1) - 2025-05-08

### Added

- Implement offline mode ([#68](https://github.com/IBM/appconfiguration-rust-sdk/pull/68))
- Support non-ssl http backends ([#57](https://github.com/IBM/appconfiguration-rust-sdk/pull/57))
- New `ConfigurationId` to group together a configuration identifier ([#54](https://github.com/IBM/appconfiguration-rust-sdk/pull/54))
- Add `AppConfigurationClient` implementation for offline mode ([#52](https://github.com/IBM/appconfiguration-rust-sdk/pull/52))

### Other

- *(deps)* update rust crate rstest to 0.25.0 ([#72](https://github.com/IBM/appconfiguration-rust-sdk/pull/72))
- Configure Renovate ([#71](https://github.com/IBM/appconfiguration-rust-sdk/pull/71))
- Merge branch 'main' into metering
- better doc
- comment
- make test work (some renames on the way to make function signatures
- make test more robust
- test for communicating the segment back from segment eval
- Merge branch 'main' into refact/hide-json-models
- Add integration tests ([#59](https://github.com/IBM/appconfiguration-rust-sdk/pull/59))
- Add integration tests using a mocked server ([#58](https://github.com/IBM/appconfiguration-rust-sdk/pull/58))
- Improve naming
- Use the `AppConfigurationOffline` implementation in testing ([#53](https://github.com/IBM/appconfiguration-rust-sdk/pull/53))
- Create function to collect Segments for segment rules ([#51](https://github.com/IBM/appconfiguration-rust-sdk/pull/51))
- Order `TargetingRule`s in properties and features just once ([#50](https://github.com/IBM/appconfiguration-rust-sdk/pull/50))
- Add (current) maintainers as authors ([#48](https://github.com/IBM/appconfiguration-rust-sdk/pull/48))
