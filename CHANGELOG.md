# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.3] - 2025-09-03

### Added
- `get_ref()` method for accessing the property value without cloning
- `update()` method for modifying values based on the current state
- `update_async()` method for asynchronous value updates
- `try_unsubscribe()` method that ignores non-existent observers
- `observer_count()` method to check the number of active observers
- `subscribe_async_filtered()` method combining filtering with async notification
- `map()` method to create derived properties with transformations
- `Default` trait implementation for `ObservableProperty<T: Default>`
- From/Into conversions between `ObserverId` and `usize` for backward compatibility

### Changed
- Enhanced error handling using `thiserror` for more idiomatic error definitions
- Replaced `std::sync::RwLock` with `parking_lot::RwLock` for better performance
- Made `ObserverId` a proper newtype struct for improved type safety
- Improved `Debug` implementation to include observer count
- Fixed documentation examples to use consistent error handling

### Optimized
- Reduced lock contention with more efficient synchronization primitives
- Improved performance of observer notifications
- More efficient memory usage in read-heavy scenarios

## [0.1.2] - 2025-08-31

### Fixed
- Corrected panic handling in async observer tasks
- Fixed edge case in error propagation for async property updates
- Improved documentation for error handling

## [0.1.1] - 2025-08-29

### Added
- Better error messages for lock acquisition failures
- Additional unit tests for edge cases

### Fixed
- Fixed race condition in observer notification
- Corrected documentation examples

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
