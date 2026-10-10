# Observable Property with Tokio

[![Crates.io](https://img.shields.io/crates/v/observable-property-tokio.svg)](https://crates.io/crates/observable-property-tokio)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

A thread-safe, async-compatible observable property implementation for Rust that allows you to observe changes to values using Tokio for asynchronous operations. This crate is inspired by the observer pattern and is designed to work seamlessly in multi-threaded environments.

## ⚠️ Important Disclaimer

**This crate is provided "as is", without warranty of any kind, express or implied.** The authors and contributors are not responsible for any damages or liability arising from the use of this software. While efforts have been made to ensure the crate functions correctly, it may contain bugs or issues in certain scenarios.

### Operational considerations

- Test the crate with your workload before adopting it in production; this project does not provide a production-readiness guarantee.
- `set()` dispatches async observers as detached Tokio tasks. Use `set_async()` or `update_async()` when the caller must wait for async observer completion or receive task join errors.
- Notifications are enqueued in value-commit order and dispatched FIFO, so mapped values cannot be overwritten by an older notification. If another caller is already dispatching, a synchronous `set()` or `update()` queues its notification and can return before observers receive it.
- Async observer batches are started in FIFO order, but batches run concurrently and can complete out of order. Observers within a batch also run concurrently. This allows an async observer to await a nested update to the same property.
- Each property has a limit of 256 queued or running notifications. `set()`, `update()`, `set_async()`, and `update_async()` return `PropertyError::Backpressure` without committing the update when the limit is reached; callers can retry after outstanding notifications finish. If a mapped property is under pressure, its current value still tracks its source and pending mapped notifications are coalesced to the latest value, with at most one such coalesced notification queued per mapped property.
- `set_async()` and `update_async()` wait for their update's async observers. Synchronous observers run on the dispatcher task.
- `update()` and `update_async()` run the update closure under the property's write lock to make updates atomic. Do not call back into the same property from that closure.
- `clear_observers()` and `shutdown()` remove registered observers but do not cancel async observer work that has already been queued or started.
- Each async observer runs in its own Tokio task; high update rates or large observer counts can add task scheduling and cloning overhead. Synchronous setters need to be called in a Tokio runtime to dispatch async observers.
- The `map()` observer is retained only while at least one clone of the derived property remains alive.

**Performance characteristics may vary** depending on system configuration, observer complexity, and concurrency patterns. The observer pattern implementation may introduce overhead in systems with very high frequency property changes or large numbers of observers.

By using this crate, you acknowledge that you have read and understood this disclaimer.

## 🚀 Features

- **Thread-safe**: Uses `Arc<RwLock<>>` for safe concurrent access with optimized locking
- **Observer pattern**: Subscribe to property changes with callbacks
- **Filtered observers**: Only notify when specific conditions are met
- **Async notifications**: Async observers can be awaited with `set_async()` and `update_async()`
- **Panic isolation**: Observer panics don't crash the system
- **Type-safe**: Generic implementation works with any `Clone + Send + Sync` type
- **Error reporting**: Async observer task failures are returned from async update methods
- **Resource management**: Built-in observer cleanup methods and RAII subscription tokens
- **Derived-property cleanup**: Dropping the final mapped property unregisters its source observer

## 📦 Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
observable-property-tokio = "0.4.0"
tokio = { version = "1.53.2", features = ["rt", "rt-multi-thread", "macros", "time", "sync"] }
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
#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    let property = ObservableProperty::new(0);

    // Subscribe with an async handler
    property.subscribe_async(|old, new| async move {
        // Perform async work
        println!("Async observer: {} -> {}", old, new);
    })?;

    property.set_async(42).await?;

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

## 📚 Examples

The crate includes several examples demonstrating different usage patterns:

- [`basic_usage.rs`](examples/basic_usage.rs) - Simple property observation
- [`filtered_observers.rs`](examples/filtered_observers.rs) - Conditional observers
- [`async_observers.rs`](examples/async_observers.rs) - Asynchronous observer handlers
- [`multi_threading.rs`](examples/multi_threading.rs) - Concurrent access patterns
- [`subscription_token.rs`](examples/subscription_token.rs) - RAII-style automatic subscription cleanup
- [`property_mapping.rs`](examples/property_mapping.rs) - Creating derived properties with transformations
- [`complex_data_type.rs`](examples/complex_data_type.rs) - Using with complex data structures
- [`reference_and_async_filtered.rs`](examples/reference_and_async_filtered.rs) - Non-cloning access and async filtered subscriptions

Run examples with:

```bash
cargo run --example basic_usage
cargo run --example subscription_token
cargo run --example property_mapping
# ... and so on for other examples
```

## 🛠️ API Reference

### Core Types

- `ObservableProperty<T>` - The main observable property type
- `PropertyError` - Error type returned by all operations
- `Observer<T>` - Type alias for observer functions: `Arc<dyn Fn(&T, &T) + Send + Sync>`
- `ObserverId` - Unique identifier for observers
- `Subscription<T>` - RAII subscription handle for automatic cleanup

### Key Methods

- `new(initial_value: T)` - Create a new observable property
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
- `shutdown() -> Result<(), PropertyError>` - Remove registered observers
- `map<U, F>(transform: F) -> Result<ObservableProperty<U>, PropertyError>` - Create derived property

## ⚡ Performance Considerations

- **Observer Count**: Async observers are each called in a separate Tokio task by `set_async()`, which provides concurrency but may have overhead for many observers
- **Update Frequency**: High-frequency updates may benefit from batching or debouncing at the application level
- **Memory Usage**: Observers are stored as `Arc<dyn Fn>` which has some memory overhead
- **Lock Contention**: Uses `RwLock` which allows multiple readers but exclusive writers
- **Resource Cleanup**: Dropping the final mapped property unregisters its source observer. Clear other subscriptions explicitly or use RAII subscription tokens.
- **Task Management**: `set_async()` and `update_async()` await async observer tasks; `set()` and `update()` detach async observer tasks.

## ✅ Validation

Continuous integration tests the declared Rust 1.89 minimum and stable, checks formatting and Clippy warnings, builds the API documentation, and verifies package contents. These checks do not replace application-specific testing or provide a production-readiness guarantee.

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

## License

Licensed under either

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)

* MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)



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
