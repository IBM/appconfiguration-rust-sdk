# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-rc.1](https://github.com/IBM/appconfiguration-rust-sdk/compare/v0.1.0-rc.0...v0.1.0-rc.1) - 2025-02-04

### Added

- Support non-ssl http backends (#57)
- New `ConfigurationId` to group together a configuration identifier (#54)
- Add `AppConfigurationClient` implementation for offline mode (#52)

### Other

- Add integration tests (#59)
- Add integration tests using a mocked server (#58)
- Improve naming
- Use the `AppConfigurationOffline` implementation in testing (#53)
- Create function to collect Segments for segment rules (#51)
- Order `TargetingRule`s in properties and features just once (#50)
- Add (current) maintainers as authors ([#48](https://github.com/IBM/appconfiguration-rust-sdk/pull/48))
