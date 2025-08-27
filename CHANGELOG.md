# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2025-08-27

### Added
- Initial release of observable-property-tokio
- Core `ObservableProperty<T>` implementation with thread-safe operations
- Synchronous observer subscription with `subscribe()`
- Asynchronous observer subscription with `subscribe_async()`
- Filtered observer subscription with `subscribe_filtered()`
- Synchronous property updates with `set()`
- Asynchronous property updates with `set_async()`
- Comprehensive error handling with `PropertyError` enum
- Observer unsubscription functionality
- Panic isolation for observer functions
- Generic implementation supporting any `Clone + Send + Sync + 'static` type
- Complete documentation with examples
- Four example programs demonstrating different usage patterns
- MIT license
- Comprehensive test suite with async testing

### Features
- Thread-safe concurrent access using `Arc<RwLock<>>`
- Tokio integration for async operations
- Observer pattern implementation
- Filtered observers for conditional notifications
- Non-blocking async observer execution
- Proper error handling and recovery
- Type-safe generic implementation

[Unreleased]: https://github.com/yourusername/observable-property-tokio/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/yourusername/observable-property-tokio/releases/tag/v0.1.0
