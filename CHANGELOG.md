# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2025-09-29

### Added
- `clear_observers()` method for removing all registered observers at once
- `shutdown()` method for comprehensive cleanup operations
- Comprehensive test coverage for cleanup methods (`test_cleanup_methods`)
- Enhanced documentation examples showing resource management patterns

### Fixed
- **Critical**: Fixed memory leak in `set_async()` method by properly awaiting spawned tasks
- **Critical**: Eliminated potential panics in `map()` method by replacing `expect()` calls with proper error handling
- Changed `map()` method return type from `ObservableProperty<U>` to `Result<ObservableProperty<U>, PropertyError>` for consistent error handling
- All observer tasks are now properly awaited to prevent resource leaks in long-running applications

### Changed
- Enhanced production readiness with proper resource management
- Improved error propagation in `map()` method - no more unwrap/expect calls that could panic
- Updated README.md with resource management examples and recent improvements section
- All async operations now guarantee task completion before returning

### Security
- Eliminated all potential panic points in public API methods
- Improved resource cleanup to prevent memory leaks in production environments

### Breaking Changes
- `map()` method now returns `Result<ObservableProperty<U>, PropertyError>` instead of `ObservableProperty<U>`
  - **Migration**: Wrap existing `map()` calls with `?` or `.unwrap()` if error handling is not needed
  - **Example**: Change `let derived = property.map(|x| x * 2);` to `let derived = property.map(|x| x * 2)?;`

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
