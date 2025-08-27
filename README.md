# Observable Property with Tokio

[![Crates.io](https://img.shields.io/crates/v/observable-property-tokio.svg)](https://crates.io/crates/observable-property-tokio)
[![Documentation](https://docs.rs/observable-property-tokio/badge.svg)](https://docs.rs/observable-property-tokio)
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

**Performance characteristics may vary** depending on system configuration, observer complexity, and concurrency patterns. The observer pattern implementation may introduce overhead in systems with very high frequency property changes or large numbers of observers.

By using this crate, you acknowledge that you have read and understood this disclaimer.

## 🚀 Features

- **Thread-safe**: Uses `Arc<RwLock<>>` for safe concurrent access
- **Observer pattern**: Subscribe to property changes with callbacks
- **Filtered observers**: Only notify when specific conditions are met
- **Async notifications**: Non-blocking observer notifications with Tokio tasks
- **Panic isolation**: Observer panics don't crash the system
- **Type-safe**: Generic implementation works with any `Clone + Send + Sync` type
- **Proper error handling**: All operations return `Result` types instead of panicking

## 📦 Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
observable-property-tokio = "0.1.0"
tokio = { version = "1.36", features = ["rt", "rt-multi-thread", "macros", "time"] }
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

## 📚 Examples

The crate includes several examples demonstrating different usage patterns:

- [`basic_usage.rs`](examples/basic_usage.rs) - Simple property observation
- [`filtered_observers.rs`](examples/filtered_observers.rs) - Conditional observers
- [`async_observers.rs`](examples/async_observers.rs) - Asynchronous observer handlers
- [`multi_threading.rs`](examples/multi_threading.rs) - Concurrent access patterns

Run examples with:

```bash
cargo run --example basic_usage
cargo run --example filtered_observers
cargo run --example async_observers
cargo run --example multi_threading
```

## 🛠️ API Reference

### Core Types

- `ObservableProperty<T>` - The main observable property type
- `PropertyError` - Error type returned by all operations
- `Observer<T>` - Type alias for observer functions: `Arc<dyn Fn(&T, &T) + Send + Sync>`
- `ObserverId` - Unique identifier for observers

### Key Methods

- `new(initial_value: T)` - Create a new observable property
- `get() -> Result<T, PropertyError>` - Get current value
- `set(new_value: T) -> Result<(), PropertyError>` - Set value synchronously
- `set_async(new_value: T) -> Result<(), PropertyError>` - Set value asynchronously
- `subscribe(observer: Observer<T>) -> Result<ObserverId, PropertyError>` - Add observer
- `subscribe_async<F, Fut>(handler: F) -> Result<ObserverId, PropertyError>` - Add async observer
- `subscribe_filtered<F>(observer: Observer<T>, filter: F) -> Result<ObserverId, PropertyError>` - Add filtered observer
- `unsubscribe(id: ObserverId) -> Result<bool, PropertyError>` - Remove observer

## ⚡ Performance Considerations

- **Observer Count**: Each observer is called in a separate Tokio task for `set_async()`, which provides good isolation but may have overhead for many observers
- **Update Frequency**: High-frequency updates may benefit from batching or debouncing at the application level
- **Memory Usage**: Observers are stored as `Arc<dyn Fn>` which has some memory overhead
- **Lock Contention**: Uses `RwLock` which allows multiple readers but exclusive writers

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🔗 Related Projects

This crate is a rework of the original `observable-property` crate to use Tokio instead of `std::thread` for better async compatibility.

## 📞 Support

If you encounter any issues or have questions:

1. Check the [documentation](https://docs.rs/observable-property-tokio)
2. Look at the [examples](examples/)
3. Search existing [issues](https://github.com/snoekiede/ObservablePropertiesTokio/issues)
4. Create a new issue if needed

---

**Remember**: This software comes with no warranty. Test thoroughly before production use.
