# CHANGELOG

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [UNRELEASED]

### Added

### Changed

### Fixed

### Removed

## [1.0.1] - 2025-06-29

### Added
- Add CHANGELOG.md

### Changed
- dbus/org.pinenote.Ebc1: Add a delay to `set_driver_mode` (up to 5s) where
  we're polling the value for changes.
- dbus/org.pinenote.Ebc1: Check the current driver mode, and fails if
   it's not one of Normal or Fast.

### Fixed
- packaging: SystemD Unit now properly support DBus activation and starts with
  the graphical-session

### Removed

## [1.0.0] - 2025-06-23

[UNRELEASED]: https://git.sr.ht/~phantomas/pinenote-service/tree/main
[1.0.1]: https://git.sr.ht/~phantomas/pinenote-service/refs/v1.0.1
[1.0.0]: https://git.sr.ht/~phantomas/pinenote-service/refs/v1.0.0
