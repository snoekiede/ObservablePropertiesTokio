//! # Observable Property with Tokio
//!
//! A thread-safe, async-compatible observable property implementation for Rust that allows you to
//! observe changes to values using Tokio for asynchronous operations.
//!
//! ## Features
//!
//! - **Thread-safe**: Uses `Arc<RwLock<>>` for safe concurrent access
//! - **Observer pattern**: Subscribe to property changes with callbacks
//! - **Filtered observers**: Only notify when specific conditions are met
//! - **Async notifications**: Non-blocking observer notifications with Tokio tasks
//! - **Panic isolation**: Observer panics don't crash the system
//! - **Type-safe**: Generic implementation works with any `Clone + Send + Sync` type
//!
//! ## Quick Start
//!
//! ```rust
//! use observable_property_tokio::ObservableProperty;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), observable_property_tokio::PropertyError> {
//!     // Create an observable property
//!     let property = ObservableProperty::new(42);
//!
//!     // Subscribe to changes
//!     let observer_id = property.subscribe(Arc::new(|old_value, new_value| {
//!         println!("Value changed from {} to {}", old_value, new_value);
//!     })).map_err(|e| {
//!         eprintln!("Failed to subscribe: {}", e);
//!         e
//!     })?;
//!
//!     // Change the value (triggers observer)
//!     property.set(100).map_err(|e| {
//!         eprintln!("Failed to set value: {}", e);
//!         e
//!     })?;
//!
//!     // For async notification (uses Tokio)
//!     property.set_async(200).await.map_err(|e| {
//!         eprintln!("Failed to set value asynchronously: {}", e);
//!         e
//!     })?;
//!
//!     // Unsubscribe when done
//!     property.unsubscribe(observer_id).map_err(|e| {
//!         eprintln!("Failed to unsubscribe: {}", e);
//!         e
//!     })?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Multi-threading Example with Tokio
//!
//! ```rust
//! use observable_property_tokio::ObservableProperty;
//! use std::sync::Arc;
//! use tokio::task;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), observable_property_tokio::PropertyError> {
//!     let property = Arc::new(ObservableProperty::new(0));
//!     let property_clone = property.clone();
//!
//!     // Subscribe from one task
//!     property.subscribe(Arc::new(|old, new| {
//!         println!("Value changed: {} -> {}", old, new);
//!     })).map_err(|e| {
//!         eprintln!("Failed to subscribe: {}", e);
//!         e
//!     })?;
//!
//!     // Modify from another task
//!     task::spawn(async move {
//!         if let Err(e) = property_clone.set(42) {
//!             eprintln!("Failed to set value: {}", e);
//!         }
//!     }).await.expect("Task panicked");
//!
//!     Ok(())
//! }
//! ```

use std::collections::HashMap;
use std::fmt;
use std::panic;
use std::sync::{Arc, RwLock};
use tokio::task;

/// Errors that can occur when working with ObservableProperty
#[derive(Debug, Clone)]
pub enum PropertyError {
    /// Failed to acquire a read lock on the property
    ReadLockError {
        /// Context describing what operation was being attempted
        context: String
    },
    /// Failed to acquire a write lock on the property  
    WriteLockError {
        /// Context describing what operation was being attempted
        context: String
    },
    /// Attempted to unsubscribe an observer that doesn't exist
    ObserverNotFound {
        /// The ID of the observer that wasn't found
        id: usize
    },
    /// The property's lock has been poisoned due to a panic in another thread
    PoisonedLock,
    /// An observer function encountered an error during execution
    ObserverError {
        /// Description of what went wrong
        reason: String
    },
    /// A Tokio-related error occurred
    TokioError {
        /// Description of what went wrong
        reason: String
    },
}

impl fmt::Display for PropertyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PropertyError::ReadLockError { context } => {
                write!(f, "Failed to acquire read lock: {}", context)
            }
            PropertyError::WriteLockError { context } => {
                write!(f, "Failed to acquire write lock: {}", context)
            }
            PropertyError::ObserverNotFound { id } => {
                write!(f, "Observer with ID {} not found", id)
            }
            PropertyError::PoisonedLock => {
                write!(
                    f,
                    "Property is in a poisoned state due to a panic in another thread"
                )
            }
            PropertyError::ObserverError { reason } => {
                write!(f, "Observer execution failed: {}", reason)
            }
            PropertyError::TokioError { reason } => {
                write!(f, "Tokio runtime error: {}", reason)
            }
        }
    }
}

impl std::error::Error for PropertyError {}

/// Function type for observers that get called when property values change
pub type Observer<T> = Arc<dyn Fn(&T, &T) + Send + Sync>;

/// Unique identifier for registered observers
pub type ObserverId = usize;

/// A thread-safe observable property that notifies observers when its value changes
///
/// This type wraps a value of type `T` and allows multiple observers to be notified
/// whenever the value is modified. All operations are thread-safe and can be called
/// from multiple threads concurrently. Asynchronous operations are powered by Tokio.
///
/// # Type Requirements
///
/// The generic type `T` must implement:
/// - `Clone`: Required for returning values and passing them to observers
/// - `Send`: Required for transferring between threads
/// - `Sync`: Required for concurrent access from multiple threads  
/// - `'static`: Required for observer callbacks that may outlive the original scope
///
/// # Examples
///
/// ```rust
/// use observable_property_tokio::ObservableProperty;
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
///     let property = ObservableProperty::new("initial".to_string());
///
///     let observer_id = property.subscribe(Arc::new(|old, new| {
///         println!("Changed from '{}' to '{}'", old, new);
///     }))?;
///
///     property.set("updated".to_string())?; // Prints: Changed from 'initial' to 'updated'
///
///     // Async version
///     property.set_async("async update".to_string()).await?;
///
///     property.unsubscribe(observer_id)?;
///
///     Ok(())
/// }
/// ```
pub struct ObservableProperty<T> {
    inner: Arc<RwLock<InnerProperty<T>>>,
}

struct InnerProperty<T> {
    value: T,
    observers: HashMap<ObserverId, Observer<T>>,
    next_id: ObserverId,
}

impl<T: Clone + Send + Sync + 'static> ObservableProperty<T> {
    /// Creates a new observable property with the given initial value
    ///
    /// # Arguments
    ///
    /// * `initial_value` - The starting value for this property
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    ///
    /// let property = ObservableProperty::new(42);
    /// match property.get() {
    ///     Ok(value) => assert_eq!(value, 42),
    ///     Err(e) => eprintln!("Failed to get property value: {}", e),
    /// }
    /// ```
    pub fn new(initial_value: T) -> Self {
        Self {
            inner: Arc::new(RwLock::new(InnerProperty {
                value: initial_value,
                observers: HashMap::new(),
                next_id: 0,
            })),
        }
    }

    /// Gets the current value of the property
    ///
    /// This method acquires a read lock, which allows multiple concurrent readers
    /// but will block if a writer currently holds the lock.
    ///
    /// # Returns
    ///
    /// `Ok(T)` containing a clone of the current value, or `Err(PropertyError)`
    /// if the lock is poisoned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    ///
    /// let property = ObservableProperty::new("hello".to_string());
    /// match property.get() {
    ///     Ok(value) => assert_eq!(value, "hello"),
    ///     Err(e) => eprintln!("Failed to get property value: {}", e),
    /// }
    /// ```
    pub fn get(&self) -> Result<T, PropertyError> {
        self.inner
            .read()
            .map(|prop| prop.value.clone())
            .map_err(|_| PropertyError::PoisonedLock)
    }

    /// Sets the property to a new value and notifies all observers
    ///
    /// This method will:
    /// 1. Acquire a write lock (blocking other readers/writers)
    /// 2. Update the value and capture a snapshot of observers
    /// 3. Release the lock
    /// 4. Notify all observers sequentially with the old and new values
    ///
    /// Observer notifications are wrapped in panic recovery to prevent one
    /// misbehaving observer from affecting others.
    ///
    /// # Arguments
    ///
    /// * `new_value` - The new value to set
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or `Err(PropertyError)` if the lock is poisoned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new(10);
    ///
    ///     property.subscribe(Arc::new(|old, new| {
    ///         println!("Value changed from {} to {}", old, new);
    ///     }))?;
    ///
    ///     property.set(20)?; // Triggers observer notification
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn set(&self, new_value: T) -> Result<(), PropertyError> {
        let (old_value, observers_snapshot) = {
            let mut prop = self
                .inner
                .write()
                .map_err(|_| PropertyError::WriteLockError {
                    context: "setting property value".to_string(),
                })?;

            let old_value = prop.value.clone();
            prop.value = new_value.clone();
            let observers_snapshot: Vec<Observer<T>> = prop.observers.values().cloned().collect();
            (old_value, observers_snapshot)
        };

        for observer in observers_snapshot {
            if let Err(e) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                observer(&old_value, &new_value);
            })) {
                eprintln!("Observer panic: {:?}", e);
            }
        }

        Ok(())
    }

    /// Sets the property to a new value and notifies observers asynchronously using Tokio tasks
    ///
    /// This method is similar to `set()` but spawns observers in individual Tokio tasks
    /// for non-blocking operation. This is useful when observers might perform
    /// time-consuming operations.
    ///
    /// Unlike the original implementation that used batching with std::thread,
    /// this version spawns a separate Tokio task for each observer, taking advantage of
    /// Tokio's efficient task scheduling system designed for many concurrent tasks.
    ///
    /// # Arguments
    ///
    /// * `new_value` - The new value to set
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or `Err(PropertyError)` if the lock is poisoned.
    /// Note that this only indicates the property was updated successfully;
    /// observer execution happens asynchronously.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    /// use tokio::time::{sleep, Duration};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new(0);
    ///
    ///     property.subscribe(Arc::new(move |old, new| {
    ///         // This observer does slow work but won't block the caller
    ///         println!("Slow observer: {} -> {}", old, new);
    ///     }))?;
    ///
    ///     // This returns immediately even though observer may be slow
    ///     property.set_async(42).await?;
    ///
    ///     // Give time for observers to run
    ///     sleep(Duration::from_millis(10)).await;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn set_async(&self, new_value: T) -> Result<(), PropertyError> {
        let (old_value, observers_snapshot) = {
            let mut prop = self
                .inner
                .write()
                .map_err(|_| PropertyError::WriteLockError {
                    context: "setting property value".to_string(),
                })?;

            let old_value = prop.value.clone();
            prop.value = new_value.clone();
            let observers_snapshot: Vec<Observer<T>> = prop.observers.values().cloned().collect();
            (old_value, observers_snapshot)
        };

        if observers_snapshot.is_empty() {
            return Ok(());
        }

        // Spawn a separate Tokio task for each observer
        // Tokio is designed to efficiently handle many small tasks
        let mut tasks = Vec::with_capacity(observers_snapshot.len());

        for observer in observers_snapshot {
            let old_val = old_value.clone();
            let new_val = new_value.clone();

            let task = task::spawn(async move {
                if let Err(e) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                    observer(&old_val, &new_val);
                })) {
                    eprintln!("Observer panic in task: {:?}", e);
                }
            });

            tasks.push(task);
        }

        // We can optionally wait for all tasks to complete
        // Uncomment if you want to ensure all observers have run before returning
        // for task in tasks {
        //     task.await.map_err(|e| PropertyError::TokioError {
        //         reason: format!("Task join error: {}", e),
        //     })?;
        // }

        Ok(())
    }

    /// Subscribes an observer function to be called when the property changes
    ///
    /// The observer function will be called with the old and new values whenever
    /// the property is modified via `set()` or `set_async()`.
    ///
    /// # Arguments
    ///
    /// * `observer` - A function wrapped in `Arc` that takes `(&T, &T)` parameters
    ///
    /// # Returns
    ///
    /// `Ok(ObserverId)` containing a unique identifier for this observer,
    /// or `Err(PropertyError)` if the lock is poisoned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new(0);
    ///
    ///     let observer_id = property.subscribe(Arc::new(|old_value, new_value| {
    ///         println!("Property changed from {} to {}", old_value, new_value);
    ///     }))?;
    ///
    ///     // Later, unsubscribe using the returned ID
    ///     property.unsubscribe(observer_id)?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn subscribe(&self, observer: Observer<T>) -> Result<ObserverId, PropertyError> {
        let mut prop = self
            .inner
            .write()
            .map_err(|_| PropertyError::WriteLockError {
                context: "subscribing observer".to_string(),
            })?;

        let id = prop.next_id;
        prop.next_id += 1;
        prop.observers.insert(id, observer);
        Ok(id)
    }

    /// Removes an observer by its ID
    ///
    /// # Arguments
    ///
    /// * `id` - The observer ID returned by `subscribe()`
    ///
    /// # Returns
    ///
    /// `Ok(bool)` where `true` means the observer was found and removed,
    /// `false` means no observer with that ID existed.
    /// Returns `Err(PropertyError)` if the lock is poisoned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new(0);
    ///     let id = property.subscribe(Arc::new(|_, _| {}))?;
    ///
    ///     let was_removed = property.unsubscribe(id)?;
    ///     assert!(was_removed); // Observer existed and was removed
    ///
    ///     let was_removed_again = property.unsubscribe(id)?;
    ///     assert!(!was_removed_again); // Observer no longer exists
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn unsubscribe(&self, id: ObserverId) -> Result<bool, PropertyError> {
        let mut prop = self
            .inner
            .write()
            .map_err(|_| PropertyError::WriteLockError {
                context: "unsubscribing observer".to_string(),
            })?;

        let was_present = prop.observers.remove(&id).is_some();
        Ok(was_present)
    }

    /// Subscribes an observer that only gets called when a filter condition is met
    ///
    /// This is useful for observing only specific types of changes, such as
    /// when a value increases or crosses a threshold.
    ///
    /// # Arguments
    ///
    /// * `observer` - The observer function to call when the filter passes
    /// * `filter` - A predicate function that receives `(old_value, new_value)` and returns `bool`
    ///
    /// # Returns
    ///
    /// `Ok(ObserverId)` for the filtered observer, or `Err(PropertyError)` if the lock is poisoned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new(0);
    ///
    ///     // Only notify when value increases
    ///     let id = property.subscribe_filtered(
    ///         Arc::new(|old, new| println!("Value increased: {} -> {}", old, new)),
    ///         |old, new| new > old
    ///     )?;
    ///
    ///     property.set(10)?; // Triggers observer (0 -> 10)
    ///     property.set(5)?;  // Does NOT trigger observer (10 -> 5)
    ///     property.set(15)?; // Triggers observer (5 -> 15)
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn subscribe_filtered<F>(
        &self,
        observer: Observer<T>,
        filter: F,
    ) -> Result<ObserverId, PropertyError>
    where
        F: Fn(&T, &T) -> bool + Send + Sync + 'static,
    {
        let filter = Arc::new(filter);
        let filtered_observer = Arc::new(move |old_val: &T, new_val: &T| {
            if filter(old_val, new_val) {
                observer(old_val, new_val);
            }
        });

        self.subscribe(filtered_observer)
    }

    /// Subscribe with an async handler that will be executed as a Tokio task
    ///
    /// This version allows you to use async functions as observers. The handler is
    /// spawned as a Tokio task whenever the property changes.
    ///
    /// # Arguments
    ///
    /// * `handler` - An async function or closure that takes old and new values
    ///
    /// # Returns
    ///
    /// `Ok(ObserverId)` containing a unique identifier for this observer,
    /// or `Err(PropertyError)` if the lock is poisoned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    /// use tokio::time::{sleep, Duration};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new(0);
    ///
    ///     // This handler can perform async operations
    ///     property.subscribe_async(|old, new| async move {
    ///         // Simulate some async work
    ///         sleep(Duration::from_millis(10)).await;
    ///         println!("Async observer: {} -> {}", old, new);
    ///     })?;
    ///
    ///     property.set_async(42).await?;
    ///
    ///     // Give time for observers to complete
    ///     sleep(Duration::from_millis(20)).await;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn subscribe_async<F, Fut>(&self, handler: F) -> Result<ObserverId, PropertyError>
    where
        F: Fn(T, T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        // Wrap the handler in an Arc so we can clone it for each invocation
        let handler = Arc::new(handler);

        let observer = Arc::new(move |old: &T, new: &T| {
            let old_val = old.clone();
            let new_val = new.clone();
            // Clone the handler so we can move it into the task
            let handler_clone = Arc::clone(&handler);

            tokio::spawn(async move {
                handler_clone(old_val, new_val).await;
            });
        });

        self.subscribe(observer)
    }
}

impl<T: Clone> Clone for ObservableProperty<T> {
    /// Creates a new reference to the same observable property
    ///
    /// This creates a new `ObservableProperty` instance that shares the same
    /// underlying data with the original. Changes made through either instance
    /// will be visible to observers subscribed through both instances.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property1 = ObservableProperty::new(42);
    ///     let property2 = property1.clone();
    ///
    ///     property2.subscribe(Arc::new(|old, new| {
    ///         println!("Observer on property2 saw change: {} -> {}", old, new);
    ///     }))?;
    ///
    ///     // This change through property1 will trigger the observer on property2
    ///     property1.set(100)?;
    ///
    ///     Ok(())
    /// }
    /// ```
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: Clone + std::fmt::Debug + Send + Sync + 'static> std::fmt::Debug for ObservableProperty<T> {
    /// Debug implementation that shows the current value if accessible
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.get() {
            Ok(value) => f.debug_struct("ObservableProperty")
                .field("value", &value)
                .field("observers_count", &"[hidden]")
                .finish(),
            Err(_) => f.debug_struct("ObservableProperty")
                .field("value", &"[inaccessible]")
                .field("observers_count", &"[hidden]")
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::time::sleep;

    // Basic tests
    #[tokio::test]
    async fn test_new_and_get() {
        let property = ObservableProperty::new(42);
        assert_eq!(property.get().unwrap(), 42);
    }

    #[tokio::test]
    async fn test_set() {
        let property = ObservableProperty::new(10);
        property.set(20).unwrap();
        assert_eq!(property.get().unwrap(), 20);
    }

    #[tokio::test]
    async fn test_set_async() {
        let property = ObservableProperty::new("hello".to_string());
        property.set_async("world".to_string()).await.unwrap();
        assert_eq!(property.get().unwrap(), "world");
    }

    #[tokio::test]
    async fn test_clone() {
        let property1 = ObservableProperty::new(100);
        let property2 = property1.clone();

        // Change through property2
        property2.set(200).unwrap();

        // Both should reflect the change
        assert_eq!(property1.get().unwrap(), 200);
        assert_eq!(property2.get().unwrap(), 200);
    }

    // Observer tests
    #[tokio::test]
    async fn test_subscribe_and_notify() {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        property.subscribe(Arc::new(move |_, _| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        })).unwrap();

        property.set(1).unwrap();
        property.set(2).unwrap();
        property.set(3).unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_subscribe_async() {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        property.subscribe_async(move |_, _| {
            let counter = counter_clone.clone();
            async move {
                // Simulate async work
                sleep(Duration::from_millis(10)).await;
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }).unwrap();

        property.set_async(1).await.unwrap();
        property.set_async(2).await.unwrap();

        // Give time for async operations to complete
        sleep(Duration::from_millis(50)).await;

        // Check counter after async operations complete
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_multiple_observers() {
        let property = ObservableProperty::new(0);
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));

        property.subscribe(Arc::new({
            let counter = counter1.clone();
            move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
        })).unwrap();

        property.subscribe(Arc::new({
            let counter = counter2.clone();
            move |_, _| { counter.fetch_add(2, Ordering::SeqCst); }
        })).unwrap();

        property.set(42).unwrap();

        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_unsubscribe() {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        let id = property.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
        })).unwrap();

        property.set(1).unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Unsubscribe and verify it was removed
        let was_removed = property.unsubscribe(id).unwrap();
        assert!(was_removed);

        // Set again, counter should not increase
        property.set(2).unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Try to unsubscribe again, should return false
        let was_removed_again = property.unsubscribe(id).unwrap();
        assert!(!was_removed_again);
    }

    // Filtered observer tests
    #[tokio::test]
    async fn test_filtered_observer() {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        property.subscribe_filtered(
            Arc::new({
                let counter = counter.clone();
                move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
            }),
            |old, new| new > old // Only trigger when value increases
        ).unwrap();

        property.set(10).unwrap(); // Should trigger (0 -> 10)
        property.set(5).unwrap();  // Should NOT trigger (10 -> 5)
        property.set(15).unwrap(); // Should trigger (5 -> 15)

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    // Concurrent access tests
    #[tokio::test]
    async fn test_concurrent_modifications() {
        let property = Arc::new(ObservableProperty::new(0));
        let final_counter = Arc::new(AtomicUsize::new(0));

        // Subscribe to track the final value
        property.subscribe(Arc::new({
            let counter = final_counter.clone();
            move |_, new| {
                counter.store(*new, Ordering::SeqCst);
            }
        })).unwrap();

        // Create multiple tasks to update the property concurrently
        let mut tasks = vec![];

        for i in 1..=5 {
            let prop = property.clone();
            let task = tokio::spawn(async move {
                prop.set(i).unwrap();
            });
            tasks.push(task);
        }

        // Wait for all tasks to complete
        for task in tasks {
            task.await.unwrap();
        }

        // Final value should be one of the set values (1-5)
        let final_value = final_counter.load(Ordering::SeqCst);
        assert!(final_value >= 1 && final_value <= 5);
    }

    // Test for observer panic handling
    #[tokio::test]
    async fn test_observer_panic_handling() {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        // First observer panics
        property.subscribe(Arc::new(|_, _| {
            panic!("This observer intentionally panics");
        })).unwrap();

        // Second observer should still run
        property.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
        })).unwrap();

        // This should not panic the test
        property.set(42).unwrap();

        // Second observer should have run
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    // Test async observer with set_async
    #[tokio::test]
    async fn test_async_observers_with_async_set() {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        // Register two types of observers - one sync, one async
        property.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
        })).unwrap();

        let counter_clone = counter.clone();
        property.subscribe_async(move |_, _| {
            let counter = counter_clone.clone();
            async move {
                sleep(Duration::from_millis(10)).await;
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }).unwrap();

        // Using set_async should notify both observers
        property.set_async(42).await.unwrap();

        // Give time for async observer to complete
        sleep(Duration::from_millis(50)).await;

        // Both observers should have incremented the counter
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    // Test for stress with many observers
    #[tokio::test]
    async fn test_many_observers() {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        // Add 100 observers
        for _ in 0..100 {
            property.subscribe(Arc::new({
                let counter = counter.clone();
                move |_, _| {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })).unwrap();
        }

        // Trigger all observers
        property.set_async(999).await.unwrap();

        // Wait for all to complete
        sleep(Duration::from_millis(100)).await;

        // All 100 observers should have incremented the counter
        assert_eq!(counter.load(Ordering::SeqCst), 100);
    }

    // Test for correct old and new values in observers
    #[tokio::test]
    async fn test_observer_receives_correct_values() {
        let property = ObservableProperty::new(100);
        let vals = Arc::new((AtomicUsize::new(0), AtomicUsize::new(0)));

        property.subscribe(Arc::new({
            let vals = vals.clone();
            move |old, new| {
                vals.0.store(*old, Ordering::SeqCst);
                vals.1.store(*new, Ordering::SeqCst);
            }
        })).unwrap();

        property.set(200).unwrap();

        assert_eq!(vals.0.load(Ordering::SeqCst), 100);
        assert_eq!(vals.1.load(Ordering::SeqCst), 200);
    }

    // Test for complex data type
    #[derive(Debug, Clone, PartialEq)]
    struct Person {
        name: String,
        age: u32,
    }

    #[tokio::test]
    async fn test_complex_data_type() {
        let person1 = Person {
            name: "Alice".to_string(),
            age: 30,
        };

        let person2 = Person {
            name: "Bob".to_string(),
            age: 25,
        };

        let property = ObservableProperty::new(person1.clone());
        assert_eq!(property.get().unwrap(), person1);

        let name_changes = Arc::new(AtomicUsize::new(0));

        property.subscribe_filtered(
            Arc::new({
                let counter = name_changes.clone();
                move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
            }),
            |old, new| old.name != new.name // Only notify on name changes
        ).unwrap();

        // Update age only - shouldn't trigger
        let mut person3 = person1.clone();
        person3.age = 31;
        property.set(person3).unwrap();
        assert_eq!(name_changes.load(Ordering::SeqCst), 0);

        // Update name - should trigger
        property.set(person2).unwrap();
        assert_eq!(name_changes.load(Ordering::SeqCst), 1);
    }

    // Test optional wait for observers
    #[tokio::test]
    async fn test_waiting_for_observers() {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        // Clone counter before moving it into the closure
        let counter_for_observer = counter.clone();

        // Add a regular observer that includes async work directly
        // instead of using subscribe_async which spawns separate tasks
        property.subscribe(Arc::new(move |_, _| {
            let counter = counter_for_observer.clone();
            // Block on the async work directly in the observer
            let rt = tokio::runtime::Handle::current();
            rt.spawn(async move {
                sleep(Duration::from_millis(50)).await;
                counter.fetch_add(1, Ordering::SeqCst);
            });
        })).unwrap();

        // Use custom implementation of set_async that waits for observers AND their spawned tasks
        async fn set_async_and_wait<T: Clone + Send + Sync + 'static>(
            property: &ObservableProperty<T>,
            new_value: T
        ) -> Result<(), PropertyError> {
            let (old_value, observers_snapshot) = {
                let mut prop = property
                    .inner
                    .write()
                    .map_err(|_| PropertyError::WriteLockError {
                        context: "setting property value".to_string(),
                    })?;

                let old_value = prop.value.clone();
                prop.value = new_value.clone();
                let observers_snapshot: Vec<Observer<T>> = prop.observers.values().cloned().collect();
                (old_value, observers_snapshot)
            };

            if observers_snapshot.is_empty() {
                return Ok(());
            }

            let mut tasks = Vec::with_capacity(observers_snapshot.len());

            for observer in observers_snapshot {
                let old_val = old_value.clone();
                let new_val = new_value.clone();

                let task = task::spawn(async move {
                    if let Err(e) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                        observer(&old_val, &new_val);
                    })) {
                        eprintln!("Observer panic in task: {:?}", e);
                    }
                });

                tasks.push(task);
            }

            // Wait for all tasks to complete
            for task in tasks {
                task.await.map_err(|e| PropertyError::TokioError {
                    reason: format!("Task join error: {}", e),
                })?;
            }

            Ok(())
        }

        // Set value and wait for observers
        set_async_and_wait(&property, 42).await.unwrap();

        // Give additional time for any spawned tasks to complete
        sleep(Duration::from_millis(100)).await;

        // Counter should be incremented after the async work completes
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
