# Observable Property with Tokio

A thread-safe, async-compatible observable property implementation for Rust that allows you to observe changes to values using Tokio for asynchronous operations.
This crate is inspired by the observer pattern and is designed to work seamlessly in multi-threaded environments.
Also, it is a rework of my own obervable-property crate to use Tokio instead of std::thread.

## Disclaimer

This crate is provided "as is", without warranty of any kind, express or implied. The authors and contributors are not responsible for any damages or liability arising from the use of this software. While efforts have been made to ensure the crate functions correctly, it may contain bugs or issues in certain scenarios. Users should thoroughly test the crate in their specific environment before deploying to production.

Performance characteristics may vary depending on system configuration, observer complexity, and concurrency patterns. The observer pattern implementation may introduce overhead in systems with very high frequency property changes or large numbers of observers.

By using this crate, you acknowledge that you have read and understood this disclaimer.

## Status
- **Performance**: The current implementation spawns individual Tokio tasks for each observer, which may not be optimal for high-frequency updates or large numbers of observers.
- **Memory Usage**: Observer callbacks are stored as `Arc<dyn Fn>` which may have memory overhead considerations.
- **API Stability**: The API may change in future versions as the design evolves.

## Features

- **Thread-safe**: Uses `Arc<RwLock<>>` for safe concurrent access
- **Observer pattern**: Subscribe to property changes with callbacks
- **Filtered observers**: Only notify when specific conditions are met
- **Async notifications**: Non-blocking observer notifications with Tokio tasks
- **Panic isolation**: Observer panics don't crash the system
- **Type-safe**: Generic implementation works with any `Clone + Send + Sync` type

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
observable-property-tokio = "0.1.0"
tokio = { version = "1.36", features = ["rt", "rt-multi-thread", "macros", "time"] }
```

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
    let observer_id = property.subscribe_filtered(
        Arc::new(|old, new| println!("Value increased: {} -> {}", old, new)),
        |old, new| new > old
    )?;

    property.set(10)?; // Triggers observer (0 -> 10)
    property.set(5)?;  // Does NOT trigger observer (10 -> 5)
    property.set(15)?; // Triggers observer (5 -> 15)

    Ok(())
}
```

### Async Observers

```rust
use observable_property_tokio::ObservableProperty;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    let property = ObservableProperty::new(0);

    // This handler can perform async operations
    property.subscribe_async(|old, new| async move {
        // Simulate some async work
        sleep(Duration::from_millis(10)).await;
        println!("Async observer: {} -> {}", old, new);
    })?;

    property.set_async(42).await?;

    // Give time for observers to complete
    sleep(Duration::from_millis(20)).await;

    Ok(())
}
```

### Multi-threading Example

```rust
use observable_property_tokio::ObservableProperty;
use std::sync::Arc;
use tokio::task;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    let property = Arc::new(ObservableProperty::new(0));
    let property_clone = property.clone();

    // Subscribe from one task
    property.subscribe(Arc::new(|old, new| {
        println!("Value changed: {} -> {}", old, new);
    }))?;

    // Modify from another task
    task::spawn(async move {
        if let Err(e) = property_clone.set(42) {
            eprintln!("Failed to set value: {}", e);
        }
    }).await.expect("Task panicked");

    Ok(())
}
```

## Examples

Run the included examples to see the library in action:

```bash
# Basic usage
cargo run --example basic_usage

# Filtered observers
cargo run --example filtered_observers

# Async observers
cargo run --example async_observers

# Multi-threading demonstration
cargo run --example multi_threading
```

## API Documentation

### Core Methods

- `new(initial_value: T) -> Self` - Create a new observable property
- `get() -> Result<T, PropertyError>` - Get the current value
- `set(new_value: T) -> Result<(), PropertyError>` - Set value and notify observers synchronously
- `set_async(new_value: T) -> Result<(), PropertyError>` - Set value and notify observers asynchronously

### Observer Management

- `subscribe(observer: Observer<T>) -> Result<ObserverId, PropertyError>` - Subscribe to changes
- `subscribe_filtered(observer: Observer<T>, filter: F) -> Result<ObserverId, PropertyError>` - Subscribe with filter
- `subscribe_async(handler: F) -> Result<ObserverId, PropertyError>` - Subscribe with async handler
- `unsubscribe(id: ObserverId) -> Result<bool, PropertyError>` - Remove an observer

### Error Handling

The library provides comprehensive error handling through the `PropertyError` enum:

- `ReadLockError` - Failed to acquire read lock
- `WriteLockError` - Failed to acquire write lock
- `ObserverNotFound` - Observer ID not found during unsubscribe
- `PoisonedLock` - Lock poisoned due to panic in another thread
- `ObserverError` - Observer function encountered an error
- `TokioError` - Tokio runtime error

## Type Requirements

The generic type `T` must implement:
- `Clone`: Required for returning values and passing them to observers
- `Send`: Required for transferring between threads
- `Sync`: Required for concurrent access from multiple threads
- `'static`: Required for observer callbacks that may outlive the original scope

## Performance Considerations

- **Observer Execution**: Synchronous observers block the setter, while async observers are spawned as separate tasks
- **Lock Contention**: Multiple concurrent reads are allowed, but writes are exclusive
- **Memory**: Each observer is stored as an `Arc<dyn Fn>` with associated overhead
- **Task Spawning**: Async operations create new Tokio tasks which have startup costs

## Testing

Run the test suite with:

```bash
cargo test
```

The project includes comprehensive tests covering:
- Basic property operations
- Observer notifications
- Concurrent access
- Error handling
- Panic recovery
- Async operations

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](https://www.google.com/search?q=LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)

* MIT license ([LICENSE-MIT](https://www.google.com/search?q=LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contributing

This is an experimental project. Contributions, suggestions, and feedback are welcome through issues and pull requests.

## Changelog

### v0.1.0
- Initial implementation
- Basic observable property functionality
- Synchronous and asynchronous observers
- Filtered observers
- Comprehensive error handling
- Thread-safe operations with Tokio integration
