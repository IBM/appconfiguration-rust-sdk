# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-rc.1](https://github.com/IBM/appconfiguration-rust-sdk/compare/v0.1.0-rc.0...v0.1.0-rc.1) - 2025-10-06

### Added

- Add `use_private_endpoint` flag to IBM client ([#122](https://github.com/IBM/appconfiguration-rust-sdk/pull/122))
- Add new operators for segment rules ([#117](https://github.com/IBM/appconfiguration-rust-sdk/pull/117))
- Implement token refresh (base on expiration seconds) ([#96](https://github.com/IBM/appconfiguration-rust-sdk/pull/96))
- Implement a metering http client ([#88](https://github.com/IBM/appconfiguration-rust-sdk/pull/88))
- [metering] Record property and feature evaluations ([#87](https://github.com/IBM/appconfiguration-rust-sdk/pull/87))
- Metering - Collect evaluation counts and push them via server_client in batches ([#83](https://github.com/IBM/appconfiguration-rust-sdk/pull/83))
- Skeleton for Metering Task ([#77](https://github.com/IBM/appconfiguration-rust-sdk/pull/77))
- Introduce `ConfigurationProvider` trait ([#74](https://github.com/IBM/appconfiguration-rust-sdk/pull/74))
- Implement offline mode ([#68](https://github.com/IBM/appconfiguration-rust-sdk/pull/68))
- Support non-ssl http backends ([#57](https://github.com/IBM/appconfiguration-rust-sdk/pull/57))
- New `ConfigurationId` to group together a configuration identifier ([#54](https://github.com/IBM/appconfiguration-rust-sdk/pull/54))
- Add `AppConfigurationClient` implementation for offline mode ([#52](https://github.com/IBM/appconfiguration-rust-sdk/pull/52))

### Fixed

- *(deps)* update rust crate tungstenite to 0.28.0 ([#120](https://github.com/IBM/appconfiguration-rust-sdk/pull/120))
- Use correct metering endpoint ([#114](https://github.com/IBM/appconfiguration-rust-sdk/pull/114))
- Description field in Segment is optional ([#99](https://github.com/IBM/appconfiguration-rust-sdk/pull/99))
- *(deps)* update rust crate tungstenite to 0.27.0 ([#82](https://github.com/IBM/appconfiguration-rust-sdk/pull/82))
- Iterate all rules in the targeting segment ([#75](https://github.com/IBM/appconfiguration-rust-sdk/pull/75))

### Other

- Test IBMCloudTokenProvider renew network call ([#97](https://github.com/IBM/appconfiguration-rust-sdk/pull/97))
- Clarify `belong_to_segment` algorithm ([#118](https://github.com/IBM/appconfiguration-rust-sdk/pull/118))
- *(deps)* update rust crate httpmock to 0.8.0 ([#121](https://github.com/IBM/appconfiguration-rust-sdk/pull/121))
- Fix detect-secrets pre-commit ([#119](https://github.com/IBM/appconfiguration-rust-sdk/pull/119))
- Ensure we use the same hash as Node client. ([#100](https://github.com/IBM/appconfiguration-rust-sdk/pull/100))
- *(deps)* update actions/setup-python action to v6 ([#110](https://github.com/IBM/appconfiguration-rust-sdk/pull/110))
- Add crates.io badge ([#98](https://github.com/IBM/appconfiguration-rust-sdk/pull/98))
- *(deps)* bump actions/checkout from 4 to 5 ([#95](https://github.com/IBM/appconfiguration-rust-sdk/pull/95))
- [metering] Differentiate between `models` and `serialization` ([#93](https://github.com/IBM/appconfiguration-rust-sdk/pull/93))
- Move existing models to `crate::models` module ([#92](https://github.com/IBM/appconfiguration-rust-sdk/pull/92))
- Split `models.rs` into `network::serialization` and `metering::models` ([#90](https://github.com/IBM/appconfiguration-rust-sdk/pull/90))
- Fix documentation warnings ([#91](https://github.com/IBM/appconfiguration-rust-sdk/pull/91))
- *(deps)* update rust crate rstest to 0.26.0 ([#89](https://github.com/IBM/appconfiguration-rust-sdk/pull/89))
- Move metering into its own module ([#86](https://github.com/IBM/appconfiguration-rust-sdk/pull/86))
- `network` module returns `Configuration` object ([#85](https://github.com/IBM/appconfiguration-rust-sdk/pull/85))
- Hide `AppConfigurationClientHttp` (and others) from the user  ([#81](https://github.com/IBM/appconfiguration-rust-sdk/pull/81))
- Renames associated to `SegmentRule` and `TargetingRule` ([#80](https://github.com/IBM/appconfiguration-rust-sdk/pull/80))
- *(deps)* update actions/setup-python action to v5 ([#79](https://github.com/IBM/appconfiguration-rust-sdk/pull/79))
- *(deps)* update actions/checkout action to v4 ([#78](https://github.com/IBM/appconfiguration-rust-sdk/pull/78))
- Run pre-commit in PRs (all files) ([#76](https://github.com/IBM/appconfiguration-rust-sdk/pull/76))
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
