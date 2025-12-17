# Observable Property with Tokio

[![Crates.io](https://img.shields.io/crates/v/observable-property-tokio.svg)](https://crates.io/crates/observable-property-tokio)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A thread-safe, async-compatible observable property implementation for Rust that allows you to observe changes to values using Tokio for asynchronous operations. This crate is inspired by the observer pattern and is designed to work seamlessly in multi-threaded environments.

## ⚠️ Important Disclaimer

**This crate is provided "as is", without warranty of any kind, express or implied.** The authors and contributors are not responsible for any damages or liability arising from the use of this software. While efforts have been made to ensure the crate functions correctly, it may contain bugs or issues in certain scenarios.

### Key Considerations:

- **Production Use**: Users should thoroughly test the crate in their specific environment before deploying to production
- **Performance**: The current implementation spawns individual Tokio tasks for each observer, which may not be optimal for high-frequency updates or large numbers of observers
- **Memory Usage**: Observer callbacks are stored as `Arc<dyn Fn>` which may have memory overhead considerations
- **API Stability**: The API may change in future versions as the design evolves
- **Error Handling**: All operations return `Result` types - proper error handling is essential
- **Resource Cleanup**: Use `clear_observers()` or `shutdown()` methods for proper cleanup in production

**Performance characteristics may vary** depending on system configuration, observer complexity, and concurrency patterns. The observer pattern implementation may introduce overhead in systems with very high frequency property changes or large numbers of observers.

By using this crate, you acknowledge that you have read and understood this disclaimer.

## 🚀 Features

- **Thread-safe**: Uses `Arc<RwLock<>>` for safe concurrent access with optimized locking
- **Observer pattern**: Subscribe to property changes with callbacks
- **Filtered observers**: Only notify when specific conditions are met
- **Async notifications**: Non-blocking observer notifications with Tokio tasks
- **Connection pooling**: Limit concurrent async tasks to prevent resource exhaustion
- **Batching**: Reduce overhead for high-frequency updates with configurable intervals
- **Panic isolation**: Observer panics don't crash the system
- **Type-safe**: Generic implementation works with any `Clone + Send + Sync` type
- **Proper error handling**: All operations return `Result` types instead of panicking
- **Resource management**: Built-in cleanup methods for production environments
- **Memory leak prevention**: Async operations properly await task completion
- **Backpressure & Rate Limiting**: Configurable limits to prevent resource exhaustion
- **Production-ready**: Comprehensive error handling and resource cleanup

## 📦 Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
observable-property-tokio = "0.3.0"
tokio = { version = "1.48.0", features = ["rt", "rt-multi-thread", "macros", "time"] }
```

## 🔧 Quick Start

### Basic Usage

```rust
use observable_property_tokio::ObservableProperty;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    // Create an observable property
    let property = ObservableProperty::new(42);

    // Subscribe to changes
    let observer_id = property.subscribe(Arc::new(|old_value, new_value| {
        println!("Value changed from {} to {}", old_value, new_value);
    }))?;

    // Change the value (triggers observer)
    property.set(100)?;

    // For async notification (uses Tokio)
    property.set_async(200).await?;

    // Unsubscribe when done
    property.unsubscribe(observer_id)?;

    // Or clear all observers at once
    property.clear_observers()?;

    Ok(())
}
```

### Async Observers

```rust
use observable_property_tokio::ObservableProperty;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    let property = ObservableProperty::new(0);

    // Subscribe with an async handler
    property.subscribe_async(|old, new| async move {
        // Simulate async work
        sleep(Duration::from_millis(100)).await;
        println!("Async observer: {} -> {}", old, new);
    })?;

    property.set_async(42).await?;

    // Give time for async observers to complete
    sleep(Duration::from_millis(200)).await;

    Ok(())
}
```

### Filtered Observers

```rust
use observable_property_tokio::ObservableProperty;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    let property = ObservableProperty::new(0);

    // Only notify when value increases
    property.subscribe_filtered(
        Arc::new(|old, new| println!("Value increased: {} -> {}", old, new)),
        |old, new| new > old
    )?;

    property.set(10)?; // Triggers observer (0 -> 10)
    property.set(5)?;  // Does NOT trigger observer (10 -> 5)
    property.set(15)?; // Triggers observer (5 -> 15)

    Ok(())
}
```

### Property Mapping and Transformation

```rust
use observable_property_tokio::ObservableProperty;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    let original = ObservableProperty::new(42);
    
    // Create a derived property that doubles the value
    let doubled = original.map(|value| value * 2)?;
    assert_eq!(doubled.get()?, 84);
    
    // Create a derived property that converts to string
    let as_string = original.map(|value| format!("Value: {}", value))?;
    assert_eq!(as_string.get()?, "Value: 42");
    
    // When original changes, all derived properties update automatically
    original.set(10)?;
    assert_eq!(doubled.get()?, 20);
    assert_eq!(as_string.get()?, "Value: 10");
    
    Ok(())
}
```

### Multi-threading

```rust
use observable_property_tokio::ObservableProperty;
use std::sync::Arc;
use tokio::task;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let property = Arc::new(ObservableProperty::new(0));
    let property_clone = property.clone();

    // Subscribe from one task
    property.subscribe(Arc::new(|old, new| {
        println!("Value changed: {} -> {}", old, new);
    }))?;

    // Modify from another task
    let handle = task::spawn(async move {
        property_clone.set(42).map_err(|e| format!("Failed to set: {}", e))
    });

    handle.await??;
    Ok(())
}
```

### Resource Management

```rust
use observable_property_tokio::ObservableProperty;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    let property = ObservableProperty::new("active".to_string());

    // Register observers for normal operation
    property.subscribe(Arc::new(|old, new| {
        println!("Status changed: {} -> {}", old, new);
    }))?;

    property.subscribe_async(|old, new| async move {
        // Simulate async processing
        println!("Async processing: {} -> {}", old, new);
    })?;

    println!("Observer count: {}", property.observer_count());

    // Clear all observers when needed
    property.clear_observers()?;
    println!("Observer count after clear: {}", property.observer_count());

    // Or perform comprehensive shutdown
    property.shutdown()?;

    // Property can still be used after cleanup, but no observers will be notified
    property.set("inactive".to_string())?;

    Ok(())
}
```

### Backpressure and Rate Limiting

```rust
use observable_property_tokio::{ObservableProperty, PropertyConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    // Configure limits to prevent resource exhaustion
    let config = PropertyConfig {
        max_observers: 100,              // Maximum number of observers
        max_pending_notifications: 50,   // Reserved for future use
        observer_timeout_ms: 5000,       // Reserved for future use
        max_concurrent_async_tasks: 100, // Maximum concurrent async tasks (default: 100)
    };

    let property = ObservableProperty::new_with_config(0, config);

    // Subscribe observers as normal
    for i in 0..100 {
        property.subscribe(Arc::new(move |_, new| {
            println!("Observer {} notified: {}", i, new);
        }))?;
    }

    // The 101st subscription will fail with CapacityExceeded error
    match property.subscribe(Arc::new(|_, _| {})) {
        Ok(_) => println!("Subscribed successfully"),
        Err(e) => {
            eprintln!("Subscription failed: {}", e);
            eprintln!("Diagnostic: {}", e.diagnostic_info());
            // Output: CAPACITY_EXCEEDED | resource=observers | current=100 | max=100 | utilization=100.0%
        }
    }

    Ok(())
}
```

### Graceful Shutdown with Timeout

```rust
use observable_property_tokio::ObservableProperty;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    let property = ObservableProperty::new(0);

    // Add observers
    property.subscribe(Arc::new(|old, new| {
        println!("Value changed: {} -> {}", old, new);
    }))?;

    property.subscribe_async(|old, new| async move {
        println!("Async observer: {} -> {}", old, new);
    })?;

    println!("Observers: {}", property.observer_count());

    // ... application running ...

    // Graceful shutdown with timeout
    let report = property.shutdown_with_timeout(Duration::from_secs(30)).await?;

    println!("Shutdown complete:");
    println!("  - Observers cleared: {}", report.observers_cleared);
    println!("  - Duration: {:?}", report.shutdown_duration);
    println!("  - Within timeout: {}", report.completed_within_timeout);
    println!("  - Diagnostic: {}", report.diagnostic_info());

    // Monitor shutdown duration
    if report.shutdown_duration > Duration::from_secs(10) {
        eprintln!("WARNING: Shutdown took longer than expected");
    }

    Ok(())
}
```

### Connection Pooling for Async Tasks

```rust
use observable_property_tokio::{ObservableProperty, PropertyConfig};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    // Configure connection pooling to limit concurrent async tasks
    let config = PropertyConfig {
        max_observers: 1000,
        max_pending_notifications: 100,
        observer_timeout_ms: 5000,
        max_concurrent_async_tasks: 5,  // Only 5 async tasks run concurrently
    };

    let property = ObservableProperty::new_with_config(0, config);
    let concurrent_count = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));

    // Subscribe 20 async observers that each take 100ms
    for _ in 0..20 {
        let counter = Arc::clone(&concurrent_count);
        let max_counter = Arc::clone(&max_concurrent);

        property.subscribe_async(move |old, new| {
            let counter = Arc::clone(&counter);
            let max_counter = Arc::clone(&max_counter);

            async move {
                let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
                max_counter.fetch_max(current, Ordering::SeqCst);

                // Simulate async work (database query, HTTP request, etc.)
                sleep(Duration::from_millis(100)).await;

                counter.fetch_sub(1, Ordering::SeqCst);
                println!("Observer executed: {} -> {}", old, new);
            }
        })?;
    }

    // Trigger notification to all 20 observers
    property.set_async(42).await?;

    // Wait for all tasks to complete
    sleep(Duration::from_millis(500)).await;

    // Verify max concurrent was not exceeded
    let max_reached = max_concurrent.load(Ordering::SeqCst);
    println!("Max concurrent tasks: {} (limit: 5)", max_reached);
    assert!(max_reached <= 5, "Connection pool limit was respected");

    Ok(())
}
```

**Benefits of Connection Pooling:**
- **Prevents resource exhaustion**: Limits concurrent async tasks to prevent CPU/memory overload
- **Predictable performance**: Bounded concurrency ensures consistent system behavior
- **Backpressure for async**: Automatic queuing when limit is reached
- **Works with all async observers**: `subscribe_async()` and `subscribe_async_filtered()`
- **Configurable limits**: Adjust `max_concurrent_async_tasks` based on your workload

**Production Recommendations:**
- Default (100): Good for most applications
- High load (50): If you have many frequent updates
- Resource constrained (20): Limited CPU/memory environments
- Heavy async work (10): If observers do expensive I/O operations
- Testing (5): Easier to observe concurrency behavior

## 📚 Examples

The crate includes several examples demonstrating different usage patterns:

- [`basic_usage.rs`](examples/basic_usage.rs) - Simple property observation
- [`filtered_observers.rs`](examples/filtered_observers.rs) - Conditional observers
- [`async_observers.rs`](examples/async_observers.rs) - Asynchronous observer handlers
- [`multi_threading.rs`](examples/multi_threading.rs) - Concurrent access patterns
- [`batching.rs`](examples/batching.rs) - Batching for high-frequency updates with 10 scenarios
- [`backpressure.rs`](examples/backpressure.rs) - Backpressure and rate limiting with configurable limits
- [`connection_pooling.rs`](examples/connection_pooling.rs) - Connection pooling for async tasks to prevent resource exhaustion
- [`graceful_shutdown.rs`](examples/graceful_shutdown.rs) - Graceful shutdown with timeout and diagnostics
- [`subscription_token.rs`](examples/subscription_token.rs) - RAII-style automatic subscription cleanup
- [`property_mapping.rs`](examples/property_mapping.rs) - Creating derived properties with transformations
- [`complex_data_type.rs`](examples/complex_data_type.rs) - Using with complex data structures
- [`reference_and_async_filtered.rs`](examples/reference_and_async_filtered.rs) - Non-cloning access and async filtered subscriptions
- [`error_diagnostics.rs`](examples/error_diagnostics.rs) - Error diagnostic features for production monitoring

Run examples with:

```bash
cargo run --example basic_usage
cargo run --example batching
cargo run --example backpressure
cargo run --example graceful_shutdown
cargo run --example subscription_token
cargo run --example property_mapping
cargo run --example error_diagnostics
# ... and so on for other examples
```

## 🛠️ API Reference

### Core Types

- `ObservableProperty<T>` - The main observable property type
- `BatchedProperty<T>` - Batched wrapper for reducing high-frequency update overhead
- `PropertyConfig` - Configuration for backpressure and resource limits
- `BatchConfig` - Configuration for batching intervals
- `ShutdownReport` - Diagnostic report from graceful shutdown operations
- `PropertyError` - Error type returned by all operations with diagnostic capabilities
- `Observer<T>` - Type alias for observer functions: `Arc<dyn Fn(&T, &T) + Send + Sync>`
- `ObserverId` - Unique identifier for observers
- `Subscription<T>` - RAII subscription handle for automatic cleanup

### Key Methods

- `new(initial_value: T)` - Create a new observable property with default configuration
- `new_with_config(initial_value: T, config: PropertyConfig)` - Create with custom configuration
- `get() -> Result<T, PropertyError>` - Get current value (clones the value)
- `get_ref() -> impl Deref<Target = T>` - Get reference to current value (no cloning)
- `set(new_value: T) -> Result<(), PropertyError>` - Set value synchronously
- `set_async(new_value: T) -> Result<(), PropertyError>` - Set value asynchronously
- `update(F: FnOnce(T) -> T)` - Update value using a function
- `update_async(F: FnOnce(T) -> T)` - Update value asynchronously using a function
- `subscribe(observer: Observer<T>) -> Result<ObserverId, PropertyError>` - Add observer
- `subscribe_with_token(observer: Observer<T>) -> Result<Subscription<T>, PropertyError>` - Add observer with automatic cleanup
- `subscribe_async<F, Fut>(handler: F) -> Result<ObserverId, PropertyError>` - Add async observer
- `subscribe_async_with_token<F, Fut>(handler: F) -> Result<Subscription<T>, PropertyError>` - Add async observer with automatic cleanup
- `subscribe_filtered<F>(observer: Observer<T>, filter: F) -> Result<ObserverId, PropertyError>` - Add filtered observer
- `unsubscribe(id: ObserverId) -> Result<bool, PropertyError>` - Remove observer
- `observer_count() -> usize` - Get number of registered observers
- `clear_observers() -> Result<(), PropertyError>` - Remove all observers
- `shutdown() -> Result<(), PropertyError>` - Fast cleanup without waiting
- `shutdown_with_timeout(timeout: Duration) -> Result<ShutdownReport, PropertyError>` - Graceful shutdown with diagnostic report
- `map<U, F>(transform: F) -> Result<ObservableProperty<U>, PropertyError>` - Create derived property

## 🔄 Resource Management and Shutdown

### Graceful Shutdown

The crate provides two shutdown methods for different use cases:

**`shutdown()`** - Fast cleanup without waiting:
```rust
property.shutdown()?;  // Immediately clears all observers
```

**`shutdown_with_timeout()`** - Graceful shutdown with diagnostics:
```rust
let report = property.shutdown_with_timeout(Duration::from_secs(30)).await?;

// Access shutdown metrics
println!("Cleared {} observers in {:?}", 
    report.observers_cleared, 
    report.shutdown_duration);

// Check if completed within timeout
if !report.completed_within_timeout {
    eprintln!("WARNING: Shutdown exceeded timeout");
}

// Get diagnostic string for logging
log::info!("Shutdown: {}", report.diagnostic_info());
```

### ShutdownReport

The `ShutdownReport` struct provides comprehensive shutdown metrics:
- `observers_cleared: usize` - Number of observers removed
- `shutdown_duration: Duration` - Time taken for shutdown
- `completed_within_timeout: bool` - Whether shutdown finished in time
- `initiated_at_ms: u64` - Timestamp when shutdown started
- `diagnostic_info() -> String` - Formatted diagnostic string

### When to Use Each Method

- **Use `shutdown()`** for:
  - Fast application teardown
  - Unit tests
  - When you don't need metrics

- **Use `shutdown_with_timeout()`** for:
  - Production environments
  - When you need observability
  - Monitoring shutdown performance
  - Detecting slow observers
  - Compliance/audit requirements

See the [`graceful_shutdown.rs`](examples/graceful_shutdown.rs) example for comprehensive demonstrations.

## ⚡ Batching for High-Frequency Updates

### Overview

The `BatchedProperty` type reduces overhead for high-frequency property updates by batching notifications to observers. Instead of notifying observers for every single update, changes are collected and observers are notified only at regular intervals with the latest value.

### Benefits

- **Reduced CPU usage**: Fewer observer callbacks and task spawns
- **Lower memory pressure**: Minimizes allocations from frequent notifications
- **Improved throughput**: Queue operations are significantly faster than immediate notifications
- **Maintains latest state**: Only the most recent value is notified per batch

### Basic Usage

```rust
use observable_property_tokio::{BatchedProperty, BatchConfig};
use std::time::Duration;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    // Create a batched property with default 100ms interval
    let property = BatchedProperty::new(0);
    
    // Or use custom configuration
    let config = BatchConfig {
        batch_interval: Duration::from_millis(50),
    };
    let property = BatchedProperty::new_with_config(0, config);
    
    // Subscribe to batched updates
    property.subscribe(Arc::new(|old, new| {
        println!("Batched update: {} -> {}", old, new);
    }))?;
    
    // Queue multiple updates rapidly
    for i in 1..=100 {
        property.queue_update(i)?;
    }
    
    // Observers will be notified once per batch interval
    // with the latest value (100 in this case)
    
    tokio::time::sleep(Duration::from_millis(150)).await;
    
    Ok(())
}
```

### Batch Configuration Strategies

**Low-latency (50ms)**:
- Best for: Interactive UI updates
- Trade-off: More notifications, lower latency

**Balanced (100ms - default)**:
- Best for: Most applications
- Trade-off: Good balance of latency and efficiency

**High-throughput (200ms)**:
- Best for: Background data processing
- Trade-off: Fewer notifications, higher latency

**Aggressive batching (500ms+)**:
- Best for: Analytics, logging, non-critical updates
- Trade-off: Minimal notifications, significant latency

### Bypassing Batching

When you need immediate updates, bypass batching:

```rust
// Queue for batching (default behavior)
property.queue_update(value)?;

// Set immediately, bypassing batching
property.set_immediate(critical_value)?;

// Or with async notification
property.set_immediate_async(critical_value).await?;

// Manual flush of pending updates
property.flush().await?;
```

### Performance Comparison

From the [`batching.rs`](examples/batching.rs) example:

```
Unbatched: 1000 updates
  - Time: ~280µs
  - Notifications: 1000

Batched: 1000 updates  
  - Time: ~30µs
  - Notifications: 1
  - Queue speedup: ~9x faster
  - Notification reduction: 1000x
```

### Use Cases

**Real-time dashboards**:
- Metrics updating at high frequency
- Reduce notification overhead while maintaining current state

**Sensor data**:
- Handle hundreds of readings per second
- Only notify with latest value at intervals

**Game state**:
- Player position, health, score updates
- Batch rapid changes during gameplay

**Analytics/logging**:
- High-frequency event tracking
- Reduce processing overhead

### API Reference

**BatchedProperty Methods**:
- `new(value)` - Create with default 100ms interval
- `new_with_config(value, config)` - Custom configuration
- `queue_update(value)` - Queue for batching
- `set_immediate(value)` - Bypass batching (sync)
- `set_immediate_async(value)` - Bypass batching (async)
- `flush()` - Force immediate flush of pending updates
- `get()` - Get current value
- `subscribe()` / `subscribe_async()` - Add observers
- `unsubscribe()` / `clear_observers()` - Remove observers

**BatchConfig**:
- `batch_interval: Duration` - How often to flush batched updates (default: 100ms)

See the comprehensive [`batching.rs`](examples/batching.rs) example for 10 different scenarios and use cases.

## 🚦 Backpressure and Resource Management

### Configuration

The crate provides `PropertyConfig` to control resource usage and prevent exhaustion:

```rust
use observable_property_tokio::{ObservableProperty, PropertyConfig};

let config = PropertyConfig {
    max_observers: 100,              // Maximum number of observers (default: 1000)
    max_pending_notifications: 50,   // Reserved for future use (default: 100)
    observer_timeout_ms: 5000,       // Reserved for future use (default: 5000)
};

let property = ObservableProperty::new_with_config(initial_value, config);
```

### Capacity Enforcement

When the maximum number of observers is reached, additional subscriptions will fail with a `PropertyError::CapacityExceeded` error:

```rust
match property.subscribe(observer) {
    Ok(id) => {
        // Successfully subscribed
        log::info!("Observer {} subscribed", id);
    }
    Err(PropertyError::CapacityExceeded { current, max, resource }) => {
        // Handle capacity limit gracefully
        log::warn!("Cannot add observer: {}/{} {} in use", current, max, resource);
        // Consider queuing the subscription or rejecting the request
    }
    Err(e) => {
        log::error!("Subscription error: {}", e.diagnostic_info());
    }
}
```

### Benefits

- **Prevents memory exhaustion** from unlimited observer growth
- **Predictable resource usage** for capacity planning
- **Early failure detection** before system resources are depleted
- **Graceful degradation** under high load
- **Explicit resource limits** that can be tuned based on workload

See the [`backpressure.rs`](examples/backpressure.rs) example for comprehensive demonstrations.

## ⚡ Performance Considerations

- **Observer Count**: Each observer is called in a separate Tokio task for `set_async()`, which provides good isolation but may have overhead for many observers
- **Update Frequency**: High-frequency updates may benefit from batching or debouncing at the application level
- **Memory Usage**: Observers are stored as `Arc<dyn Fn>` which has some memory overhead
- **Capacity Limits**: Configure `max_observers` based on your expected load to balance flexibility and resource protection
- **Lock Contention**: Uses `RwLock` which allows multiple readers but exclusive writers
- **Resource Cleanup**: Use cleanup methods to prevent memory leaks in long-running applications
- **Task Management**: Async operations now properly await completion, preventing resource leaks

## � Error Handling and Diagnostics

The crate provides comprehensive error diagnostics for production monitoring and debugging.

### Error Types

All operations return `Result<T, PropertyError>` with detailed error variants:

- `ReadLockError` / `WriteLockError` - Lock acquisition failures with operation context and timestamps
- `LockPoisoned` - Poisoned lock detection with operation context
- `ObserverNotFound` - Observer ID not found during unsubscribe
- `ObserverPanic` - Observer function panicked with error details and observer ID
- `ObserverError` - Observer execution failure
- `TokioError` - Tokio runtime errors
- `JoinError` - Task join failures
- `CapacityExceeded` - Resource limits exceeded with utilization metrics
- `OperationTimeout` - Operation exceeded threshold with timing details
- `ShutdownInProgress` - Property is shutting down

### Diagnostic Information

Each error provides a `diagnostic_info()` method that returns structured diagnostic data:

```rust
use observable_property_tokio::{ObservableProperty, PropertyError};

let property = ObservableProperty::new(42);

match property.set(100) {
    Ok(_) => println!("Value updated"),
    Err(e) => {
        // Get human-readable error
        eprintln!("Error: {}", e);
        
        // Get structured diagnostic information for logging
        eprintln!("Diagnostic: {}", e.diagnostic_info());
        
        // Example output:
        // "OPERATION_TIMEOUT | operation=notify_observers | elapsed_ms=5500 | threshold_ms=5000 | overage_ms=500"
    }
}
```

### Helper Functions for Error Creation

The crate provides helper functions for creating errors with automatic timestamp injection:

```rust
// Automatically includes current timestamp
let error = PropertyError::read_lock_error("get_value", "lock acquisition failed");
let error = PropertyError::write_lock_error("set_value", "lock acquisition failed");
let error = PropertyError::lock_poisoned("notify", "inner lock poisoned");
let error = PropertyError::observer_panic(observer_id, "panic message");
```

### Integration with Logging

The diagnostic information is designed for easy integration with logging frameworks:

```rust
// With the log crate
log::error!("Property operation failed: {}", error.diagnostic_info());

// With tracing
tracing::error!(
    diagnostic = %error.diagnostic_info(),
    error = %error,
    "Property operation failed"
);

// With custom metrics
if let PropertyError::OperationTimeout { elapsed_ms, .. } = error {
    metrics::histogram!("property_operation_latency_ms", elapsed_ms);
}
```

See [`error_diagnostics.rs`](examples/error_diagnostics.rs) example for comprehensive demonstrations.

## �🔄 Recent Improvements

This crate has been enhanced for production readiness:

### Version 0.2.0+ Improvements
- **Memory leak prevention**: Fixed potential memory leaks in `set_async()` by properly awaiting task completion
- **Panic-safe operations**: Eliminated potential panics in `map()` method with proper error handling
- **Breaking change**: `map()` method now returns `Result<ObservableProperty<U>, PropertyError>` for enhanced safety
  - Migration: Add `?` to existing `map()` calls: `property.map(|x| x * 2)?`
- **Resource management**: Added `clear_observers()` and `shutdown()` methods for production cleanup
- **Enhanced error handling**: All operations now handle errors gracefully without unwrap/expect calls
- **Comprehensive testing**: Added tests for cleanup methods and edge cases

### Production Readiness Features
- ✅ Memory leak prevention in async operations
- ✅ Proper error propagation without panics
- ✅ Resource cleanup methods for application lifecycle
- ✅ Comprehensive test coverage including edge cases
- ✅ Thread-safe operations with proper locking
- ✅ Panic isolation for observer functions

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

## License

Licensed under either

* Apache License, Version 2.0, ([LICENSE-APACHE](https://www.google.com/search?q=LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)

* MIT license ([LICENSE-MIT](https://www.google.com/search?q=LICENSE-MIT) or <http://opensource.org/licenses/MIT>)



## 🔗 Related Projects

This crate is a rework of the original `observable-property` crate to use Tokio instead of `std::thread` for better async compatibility.

## 📞 Support

If you encounter any issues or have questions:

1. Check the [documentation](https://docs.rs/observable-property-tokio)
2. Look at the examples
3. Search existing [issues](https://github.com/snoekiede/ObservablePropertiesTokio/issues)
4. Create a new issue if needed

---

**Remember**: This software comes with no warranty. Test thoroughly before production use.
