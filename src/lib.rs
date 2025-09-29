//! # Observable Property with Tokio
//!
//! A thread-safe, async-compatible observable property implementation for Rust that allows you to
//! observe changes to values using Tokio for asynchronous operations.
//!
//! ## Features
//!
//! - **Thread-safe**: Uses `Arc<RwLock<>>` for safe concurrent access with optimized locking
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
//!     }))?;
//!
//!     // Change the value (triggers observer)
//!     property.set(100)?;
//!
//!     // For async notification (uses Tokio)
//!     property.set_async(200).await?;
//!
//!     // Unsubscribe when done
//!     property.unsubscribe(observer_id)?;
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
//!     }))?;
//!
//!     // Modify from another task
//!     task::spawn(async move {
//!         property_clone.set(42)?;
//!         Ok::<_, observable_property_tokio::PropertyError>(())
//!     }).await??;
//!
//!     Ok(())
//! }
//! ```

use std::collections::HashMap;
use std::fmt;
use std::panic;
use std::sync::Arc;
use parking_lot::RwLock;
use thiserror::Error;
use tokio::task::{self, JoinError};

/// Errors that can occur when working with ObservableProperty
#[derive(Error, Debug, Clone)]
pub enum PropertyError {
    /// Failed to acquire a read lock on the property
    #[error("Failed to acquire read lock: {context}")]
    ReadLockError {
        /// Context describing what operation was being attempted
        context: String
    },

    /// Failed to acquire a write lock on the property
    #[error("Failed to acquire write lock: {context}")]
    WriteLockError {
        /// Context describing what operation was being attempted
        context: String
    },

    /// Attempted to unsubscribe an observer that doesn't exist
    #[error("Observer with ID {id} not found")]
    ObserverNotFound {
        /// The ID of the observer that wasn't found
        id: ObserverId
    },

    /// The property's lock has been poisoned due to a panic in another thread
    #[error("Property is in a poisoned state due to a panic in another thread")]
    PoisonedLock,

    /// An observer function encountered an error during execution
    #[error("Observer execution failed: {reason}")]
    ObserverError {
        /// Description of what went wrong
        reason: String
    },

    /// A Tokio-related error occurred
    #[error("Tokio runtime error: {reason}")]
    TokioError {
        /// Description of what went wrong
        reason: String
    },

    /// A task join error occurred
    #[error("Task join error: {0}")]
    JoinError(String),
}

/// Function type for observers that get called when property values change
pub type Observer<T> = Arc<dyn Fn(&T, &T) + Send + Sync>;

/// Unique identifier for registered observers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObserverId(pub(crate) usize);

impl From<ObserverId> for usize {
    /// Convert an ObserverId to a usize
    ///
    /// This allows backward compatibility with code that expects to use
    /// the ID as a regular number.
    ///
    /// # Examples
    ///
    /// ```
    /// use observable_property_tokio::ObserverId;
    ///
    /// let id = ObserverId::from(42); // For illustration - actual IDs come from subscribe()
    /// let value: usize = id.into();
    /// assert_eq!(value, 42);
    /// ```
    fn from(id: ObserverId) -> Self {
        id.0
    }
}

impl From<usize> for ObserverId {
    /// Create an ObserverId from a usize
    ///
    /// This is primarily for backward compatibility and testing.
    /// In normal usage, IDs are created by the library through subscribe() calls.
    ///
    /// # Examples
    ///
    /// ```
    /// use observable_property_tokio::ObserverId;
    ///
    /// let id = ObserverId::from(42);
    /// ```
    fn from(value: usize) -> Self {
        ObserverId(value)
    }
}

impl fmt::Display for ObserverId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

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
    next_id: usize,
}

pub struct PropertyHandle<T: Clone + Send + Sync + 'static> {
    inner: Arc<RwLock<InnerProperty<T>>>,
}

impl<T: Clone + Send + Sync + 'static> PropertyHandle<T> {
    /// Removes an observer by its ID, ignoring if it doesn't exist
    ///
    /// This is a convenience method that doesn't return an error if the observer doesn't exist.
    ///
    /// # Arguments
    ///
    /// * `id` - The observer ID returned by `subscribe()`
    ///
    /// # Returns
    ///
    /// `true` if an observer was removed, `false` if no observer with that ID existed
    pub fn try_unsubscribe(&self, id: ObserverId) -> bool {
        let mut inner = self.inner.write();
        inner.observers.remove(&id).is_some()
    }
}

pub struct Subscription<T: Clone + Send + Sync + 'static> {
    inner: Arc<RwLock<InnerProperty<T>>>,
    id: ObserverId,
}

impl<T: Clone + Send + Sync + 'static> Drop for Subscription<T> {
    fn drop(&mut self) {
        let mut inner = self.inner.write();
        inner.observers.remove(&self.id);
    }
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
    /// assert_eq!(property.get().unwrap(), 42);
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
    /// This method acquires a read lock, which allows multiple concurrent readers.
    ///
    /// # Returns
    ///
    /// `Ok(T)` containing a clone of the current value
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    ///
    /// let property = ObservableProperty::new("hello".to_string());
    /// assert_eq!(property.get().unwrap(), "hello");
    /// ```
    pub fn get(&self) -> Result<T, PropertyError> {
        Ok(self.inner.read().value.clone())
    }

    /// Gets a reference to the current value of the property
    ///
    /// This method acquires a read lock and returns a guard that derefs to the value,
    /// allowing you to read the value without cloning it.
    ///
    /// # Returns
    ///
    /// A RAII guard that derefs to a reference of the value
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    ///
    /// let property = ObservableProperty::new("hello".to_string());
    /// let value_ref = property.get_ref();
    /// assert_eq!(*value_ref, "hello");
    /// // Lock is released when value_ref goes out of scope
    /// ```
    pub fn get_ref(&self) -> impl std::ops::Deref<Target = T> + '_ {
        parking_lot::RwLockReadGuard::map(self.inner.read(), |inner| &inner.value)
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
    /// `Ok(())` if successful
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
            let mut inner = self.inner.write();

            let old_value = inner.value.clone();
            inner.value = new_value.clone();
            let observers_snapshot: Vec<Observer<T>> = inner.observers.values().cloned().collect();
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
    /// # Arguments
    ///
    /// * `new_value` - The new value to set
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful
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
            let mut inner = self.inner.write();

            let old_value = inner.value.clone();
            inner.value = new_value.clone();
            let observers_snapshot: Vec<Observer<T>> = inner.observers.values().cloned().collect();
            (old_value, observers_snapshot)
        };

        if observers_snapshot.is_empty() {
            return Ok(());
        }

        // Spawn a separate Tokio task for each observer
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

        // Wait for all tasks to complete to prevent resource leaks
        for task in tasks {
            task.await.map_err(|e| PropertyError::JoinError(format!("Task join error: {}", e)))?;
        }

        Ok(())
    }

    /// Update the property value using a closure that has access to the current value
    ///
    /// This is a more ergonomic way to update a property based on its current value,
    /// without having to call `get()` and `set()` separately.
    ///
    /// # Arguments
    ///
    /// * `update_fn` - A function that takes the current value and returns a new value
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    ///
    /// let property = ObservableProperty::new(10);
    ///
    /// // Double the value
    /// property.update(|current| current * 2)?;
    /// assert_eq!(property.get()?, 20);
    ///
    /// // Add 5
    /// property.update(|current| current + 5)?;
    /// assert_eq!(property.get()?, 25);
    /// # Ok::<(), observable_property_tokio::PropertyError>(())
    /// ```
    pub fn update<F>(&self, update_fn: F) -> Result<(), PropertyError>
    where
        F: FnOnce(T) -> T,
    {
        let new_value = update_fn(self.get()?);
        self.set(new_value)
    }

    /// Update the property value asynchronously using a closure
    ///
    /// Like `update()` but uses `set_async()` for the update.
    ///
    /// # Arguments
    ///
    /// * `update_fn` - A function that takes the current value and returns a new value
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new("hello".to_string());
    ///
    ///     property.update_async(|current| format!("{} world", current)).await?;
    ///     assert_eq!(property.get()?, "hello world");
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn update_async<F>(&self, update_fn: F) -> Result<(), PropertyError>
    where
        F: FnOnce(T) -> T,
    {
        let new_value = update_fn(self.get()?);
        self.set_async(new_value).await
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
    /// `Ok(ObserverId)` containing a unique identifier for this observer
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
        let mut inner = self.inner.write();

        let id = ObserverId(inner.next_id);
        inner.next_id += 1;
        inner.observers.insert(id, observer);
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
    /// `Ok(())` if the observer was removed, or `Err(PropertyError::ObserverNotFound)`
    /// if no observer with that ID existed.
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
    ///     // Remove the observer
    ///     property.unsubscribe(id)?;
    ///
    ///     // Trying to remove again fails with ObserverNotFound
    ///     match property.unsubscribe(id) {
    ///         Err(observable_property_tokio::PropertyError::ObserverNotFound { .. }) => {
    ///             println!("Observer was already removed, as expected");
    ///         }
    ///         _ => panic!("Expected ObserverNotFound error"),
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn unsubscribe(&self, id: ObserverId) -> Result<(), PropertyError> {
        let mut inner = self.inner.write();

        if inner.observers.remove(&id).is_some() {
            Ok(())
        } else {
            Err(PropertyError::ObserverNotFound { id })
        }
    }

    /// Removes an observer by its ID, ignoring if it doesn't exist
    ///
    /// This is a convenience method that doesn't return an error if the observer doesn't exist.
    ///
    /// # Arguments
    ///
    /// * `id` - The observer ID returned by `subscribe()`
    ///
    /// # Returns
    ///
    /// `true` if an observer was removed, `false` if no observer with that ID existed
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
    ///     // Remove the observer
    ///     assert!(property.try_unsubscribe(id));
    ///
    ///     // Trying to remove again just returns false
    ///     assert!(!property.try_unsubscribe(id));
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn try_unsubscribe(&self, id: ObserverId) -> bool {
        let mut inner = self.inner.write();
        inner.observers.remove(&id).is_some()
    }

    /// Returns the number of active observers for this property
    ///
    /// # Returns
    ///
    /// The number of observers currently subscribed to this property
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
    ///     assert_eq!(property.observer_count(), 0);
    ///
    ///     let id1 = property.subscribe(Arc::new(|_, _| {}))?;
    ///     let id2 = property.subscribe(Arc::new(|_, _| {}))?;
    ///
    ///     assert_eq!(property.observer_count(), 2);
    ///
    ///     property.unsubscribe(id1)?;
    ///
    ///     assert_eq!(property.observer_count(), 1);
    ///
    ///     property.unsubscribe(id2)?;
    ///
    ///     assert_eq!(property.observer_count(), 0);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn observer_count(&self) -> usize {
        self.inner.read().observers.len()
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
    /// `Ok(ObserverId)` for the filtered observer
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
    /// `Ok(ObserverId)` containing a unique identifier for this observer
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

    /// Subscribe with an async handler that is filtered
    ///
    /// Combines `subscribe_filtered` and `subscribe_async` to provide an async handler
    /// that only runs when the filter condition is met.
    ///
    /// # Arguments
    ///
    /// * `handler` - An async function or closure that takes old and new values
    /// * `filter` - A predicate function that decides if the handler should be called
    ///
    /// # Returns
    ///
    /// `Ok(ObserverId)` containing a unique identifier for this observer
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use tokio::time::{sleep, Duration};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new(0);
    ///
    ///     // This async handler only runs when value increases
    ///     property.subscribe_async_filtered(
    ///         |old, new| async move {
    ///             sleep(Duration::from_millis(10)).await;
    ///             println!("Value increased: {} -> {}", old, new);
    ///         },
    ///         |old, new| new > old
    ///     )?;
    ///
    ///     property.set(10)?; // Triggers observer (0 -> 10)
    ///     property.set(5)?;  // Does NOT trigger observer (10 -> 5)
    ///     property.set(15)?; // Triggers observer (5 -> 15)
    ///
    ///     // Give time for observers to complete
    ///     sleep(Duration::from_millis(20)).await;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn subscribe_async_filtered<F, Fut, Filt>(
        &self,
        handler: F,
        filter: Filt,
    ) -> Result<ObserverId, PropertyError>
    where
        F: Fn(T, T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
        Filt: Fn(&T, &T) -> bool + Send + Sync + 'static,
    {
        let filter = Arc::new(filter);
        let handler = Arc::new(handler);

        let observer = Arc::new(move |old: &T, new: &T| {
            if filter(old, new) {
                let old_val = old.clone();
                let new_val = new.clone();
                let handler_clone = Arc::clone(&handler);

                tokio::spawn(async move {
                    handler_clone(old_val, new_val).await;
                });
            }
        });

        self.subscribe(observer)
    }

    /// Create a new ObservableProperty with transformation applied to the value
    ///
    /// This creates a derived property that tracks changes to the original property,
    /// but with a transformation applied. Changes to the original property are reflected
    /// in the derived property, but the derived property is read-only.
    ///
    /// # Arguments
    ///
    /// * `transform` - A function that converts from the source type to the target type
    ///
    /// # Returns
    ///
    /// `Ok(ObservableProperty<U>)` containing a new property that reflects the transformed value,
    /// or `Err(PropertyError)` if the initial value cannot be read or the observer cannot be subscribed
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let original = ObservableProperty::new(42);
    ///
    ///     // Create a derived property that doubles the value
    ///     let doubled = original.map(|value| value * 2)?;
    ///
    ///     assert_eq!(doubled.get()?, 84);
    ///
    ///     // When original changes, doubled reflects the transformation
    ///     original.set(10)?;
    ///     assert_eq!(doubled.get()?, 20);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn map<U, F>(&self, transform: F) -> Result<ObservableProperty<U>, PropertyError>
    where
        U: Clone + Send + Sync + 'static,
        F: Fn(&T) -> U + Send + Sync + 'static,
    {
        let transform = Arc::new(transform);
        let initial_value = transform(&self.get()?);
        let derived = ObservableProperty::new(initial_value);

        let derived_clone = derived.clone();
        self.subscribe(Arc::new(move |_, new_value| {
            let transformed = transform(new_value);
            if let Err(e) = derived_clone.set(transformed) {
                eprintln!("Failed to update derived property: {}", e);
            }
        }))?;

        Ok(derived)
    }

    /// Removes all registered observers from this property
    ///
    /// This method clears all observers that have been registered via `subscribe()`,
    /// `subscribe_async()`, `subscribe_filtered()`, or `subscribe_async_filtered()`.
    /// After calling this method, no observers will be notified of value changes
    /// until new ones are registered.
    ///
    /// This is useful for cleanup scenarios or when you need to reset the observer
    /// state without creating a new property instance.
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new(42);
    ///
    ///     // Register some observers
    ///     property.subscribe(Arc::new(|old, new| {
    ///         println!("Value changed from {} to {}", old, new);
    ///     }))?;
    ///
    ///     assert_eq!(property.observer_count(), 1);
    ///
    ///     // Clear all observers
    ///     property.clear_observers()?;
    ///     assert_eq!(property.observer_count(), 0);
    ///
    ///     // Setting value now won't trigger any observers
    ///     property.set(100)?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn clear_observers(&self) -> Result<(), PropertyError> {
        let mut inner = self.inner.write();
        inner.observers.clear();
        Ok(())
    }

    /// Performs cleanup operations on this property
    ///
    /// This method clears all registered observers, effectively shutting down
    /// the property's observer functionality. This is particularly useful in
    /// production environments where you need to ensure proper resource cleanup
    /// during application shutdown or when disposing of property instances.
    ///
    /// Currently, this method performs the same operation as `clear_observers()`,
    /// but it's provided as a separate method to allow for future expansion
    /// of cleanup operations (such as canceling pending async tasks).
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new("active".to_string());
    ///
    ///     // Register observers for normal operation
    ///     property.subscribe(Arc::new(|old, new| {
    ///         println!("Status changed: {} -> {}", old, new);
    ///     }))?;
    ///
    ///     // ... normal application usage ...
    ///
    ///     // Shutdown the property when done
    ///     property.shutdown()?;
    ///
    ///     // Property can still be used for getting/setting values,
    ///     // but no observers will be notified
    ///     property.set("inactive".to_string())?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn shutdown(&self) -> Result<(), PropertyError> {
        // Cancel any pending async observers
        self.clear_observers()
    }

    pub fn subscribe_with_token(&self, observer: Observer<T>) -> Result<Subscription<T>, PropertyError> {
        let id = self.subscribe(observer)?;
        Ok(Subscription {
            inner: Arc::clone(&self.inner),
            id
        })
    }

    pub fn subscribe_filtered_with_token<F>(
        &self,
        observer: Observer<T>,
        filter: F,
    ) -> Result<Subscription<T>, PropertyError>
    where
        F: Fn(&T, &T) -> bool + Send + Sync + 'static,
    {
        let id = self.subscribe_filtered(observer, filter)?;
        Ok(Subscription {
            inner: Arc::clone(&self.inner),
            id
        })
    }

    pub fn subscribe_async_with_token<F, Fut>(&self, handler: F) -> Result<Subscription<T>, PropertyError>
    where
        F: Fn(T, T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let id = self.subscribe_async(handler)?;
        Ok(Subscription {
            inner: Arc::clone(&self.inner),
            id
        })
    }

    pub fn subscribe_async_filtered_with_token<F, Fut, Filt>(
        &self,
        handler: F,
        filter: Filt,
    ) -> Result<Subscription<T>, PropertyError>
    where
        F: Fn(T, T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
        Filt: Fn(&T, &T) -> bool + Send + Sync + 'static,
    {
        let id = self.subscribe_async_filtered(handler, filter)?;
        Ok(Subscription {
            inner: Arc::clone(&self.inner),
            id
        })
    }

}

impl<T: Clone + Send + Sync + 'static + Default> Default for ObservableProperty<T> {
    /// Creates a new ObservableProperty with the default value for type T
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    ///
    /// let property: ObservableProperty<i32> = Default::default();
    /// assert_eq!(property.get().unwrap(), 0); // Default for i32 is 0
    /// ```
    fn default() -> Self {
        Self::new(T::default())
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

impl<T: Clone + Send + Sync + 'static> Clone for PropertyHandle<T> {
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
                .field("observers_count", &self.observer_count())
                .finish(),
            Err(_) => f.debug_struct("ObservableProperty")
                .field("value", &"[inaccessible]")
                .field("observers_count", &self.observer_count())
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
    async fn test_new_and_get() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(42);
        assert_eq!(property.get()?, 42);
        Ok(())
    }

    #[tokio::test]
    async fn test_default() {
        let property: ObservableProperty<String> = Default::default();
        assert_eq!(property.get().unwrap(), String::default());
    }

    #[tokio::test]
    async fn test_get_ref() {
        let property = ObservableProperty::new("hello".to_string());
        let value_ref = property.get_ref();
        assert_eq!(*value_ref, "hello");
    }

    #[tokio::test]
    async fn test_set() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(10);
        property.set(20)?;
        assert_eq!(property.get()?, 20);
        Ok(())
    }

    #[tokio::test]
    async fn test_update() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(10);
        property.update(|val| val * 2)?;
        assert_eq!(property.get()?, 20);
        Ok(())
    }

    #[tokio::test]
    async fn test_update_async() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(10);
        property.update_async(|val| val * 2).await?;
        assert_eq!(property.get()?, 20);
        Ok(())
    }

    #[tokio::test]
    async fn test_map() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(10);
        let derived = property.map(|val| val.to_string())?;

        assert_eq!(derived.get()?, "10");

        property.set(20)?;
        assert_eq!(derived.get()?, "20");
        Ok(())
    }

    #[tokio::test]
    async fn test_set_async() -> Result<(), PropertyError> {
        let property = ObservableProperty::new("hello".to_string());
        property.set_async("world".to_string()).await?;
        assert_eq!(property.get()?, "world");
        Ok(())
    }

    #[tokio::test]
    async fn test_clone() -> Result<(), PropertyError> {
        let property1 = ObservableProperty::new(100);
        let property2 = property1.clone();

        // Change through property2
        property2.set(200)?;

        // Both should reflect the change
        assert_eq!(property1.get()?, 200);
        assert_eq!(property2.get()?, 200);
        Ok(())
    }

    // Observer tests
    #[tokio::test]
    async fn test_subscribe_and_notify() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        property.subscribe(Arc::new(move |_, _| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }))?;

        property.set(1)?;
        property.set(2)?;
        property.set(3)?;

        assert_eq!(counter.load(Ordering::SeqCst), 3);
        Ok(())
    }

    #[tokio::test]
    async fn test_observer_count() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        assert_eq!(property.observer_count(), 0);

        let id1 = property.subscribe(Arc::new(|_, _| {}))?;
        let id2 = property.subscribe(Arc::new(|_, _| {}))?;
        assert_eq!(property.observer_count(), 2);

        property.unsubscribe(id1)?;
        assert_eq!(property.observer_count(), 1);

        property.unsubscribe(id2)?;
        assert_eq!(property.observer_count(), 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_try_unsubscribe() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let id = property.subscribe(Arc::new(|_, _| {}))?;

        assert!(property.try_unsubscribe(id));
        assert!(!property.try_unsubscribe(id));

        Ok(())
    }

    #[tokio::test]
    async fn test_subscribe_async() -> Result<(), PropertyError> {
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
        })?;

        property.set_async(1).await?;
        property.set_async(2).await?;

        // Give time for async operations to complete
        sleep(Duration::from_millis(50)).await;

        // Check counter after async operations complete
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_subscribe_async_filtered() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        property.subscribe_async_filtered(
            move |_, _| {
                let counter = counter_clone.clone();
                async move {
                    sleep(Duration::from_millis(10)).await;
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            },
            |old, new| new > old
        )?;

        property.set_async(10).await?; // Should trigger (0 -> 10)
        property.set_async(5).await?;  // Should NOT trigger (10 -> 5)
        property.set_async(15).await?; // Should trigger (5 -> 15)

        // Give time for async operations to complete
        sleep(Duration::from_millis(50)).await;

        // Only two updates should have triggered the observer
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_multiple_observers() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));

        property.subscribe(Arc::new({
            let counter = counter1.clone();
            move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
        }))?;

        property.subscribe(Arc::new({
            let counter = counter2.clone();
            move |_, _| { counter.fetch_add(2, Ordering::SeqCst); }
        }))?;

        property.set(42)?;

        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_unsubscribe() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        let id = property.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
        }))?;

        property.set(1)?;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Unsubscribe and verify it no longer receives updates
        property.unsubscribe(id)?;

        // Set again, counter should not increase
        property.set(2)?;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Try to unsubscribe again, should fail with ObserverNotFound
        match property.unsubscribe(id) {
            Err(PropertyError::ObserverNotFound { .. }) => {},
            other => panic!("Expected ObserverNotFound error, got {:?}", other),
        }

        Ok(())
    }

    // Filtered observer tests
    #[tokio::test]
    async fn test_filtered_observer() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        property.subscribe_filtered(
            Arc::new({
                let counter = counter.clone();
                move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
            }),
            |old, new| new > old // Only trigger when value increases
        )?;

        property.set(10)?; // Should trigger (0 -> 10)
        property.set(5)?;  // Should NOT trigger (10 -> 5)
        property.set(15)?; // Should trigger (5 -> 15)

        assert_eq!(counter.load(Ordering::SeqCst), 2);
        Ok(())
    }

    // Concurrent access tests
    #[tokio::test]
    async fn test_concurrent_modifications() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let property = Arc::new(ObservableProperty::new(0));
        let final_counter = Arc::new(AtomicUsize::new(0));

        // Subscribe to track the final value
        property.subscribe(Arc::new({
            let counter = final_counter.clone();
            move |_, new| {
                counter.store(*new, Ordering::SeqCst);
            }
        }))?;

        // Create multiple tasks to update the property concurrently
        let mut tasks = vec![];

        for i in 1..=5 {
            let prop = property.clone();
            let task = tokio::spawn(async move {
                prop.set(i).map_err(|e| format!("Failed to set property: {}", e))
            });
            tasks.push(task);
        }

        // Wait for all tasks to complete
        for task in tasks {
            task.await??;
        }

        // Final value should be one of the set values (1-5)
        let final_value = final_counter.load(Ordering::SeqCst);
        assert!(final_value >= 1 && final_value <= 5);
        Ok(())
    }

    // Test for observer panic handling
    #[tokio::test]
    async fn test_observer_panic_handling() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        // First observer panics
        property.subscribe(Arc::new(|_, _| {
            panic!("This observer intentionally panics");
        }))?;

        // Second observer should still run
        property.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
        }))?;

        // This should not panic the test
        property.set(42)?;

        // Second observer should have run
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        Ok(())
    }

    // More tests for new functionality
    #[tokio::test]
    async fn test_async_observers_with_async_set() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        // Register two types of observers - one sync, one async
        property.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
        }))?;

        let counter_clone = counter.clone();
        property.subscribe_async(move |_, _| {
            let counter = counter_clone.clone();
            async move {
                sleep(Duration::from_millis(10)).await;
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })?;

        // Using set_async should notify both observers
        property.set_async(42).await?;

        // Give time for async observer to complete
        sleep(Duration::from_millis(50)).await;

        // Both observers should have incremented the counter
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        Ok(())
    }

    // Test for stress with many observers
    #[tokio::test]
    async fn test_many_observers() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        // Add 100 observers
        for _ in 0..100 {
            property.subscribe(Arc::new({
                let counter = counter.clone();
                move |_, _| {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }))?;
        }

        // Trigger all observers
        property.set_async(999).await?;

        // Wait for all to complete
        sleep(Duration::from_millis(100)).await;

        // All 100 observers should have incremented the counter
        assert_eq!(counter.load(Ordering::SeqCst), 100);
        Ok(())
    }

    // Test for correct old and new values in observers
    #[tokio::test]
    async fn test_observer_receives_correct_values() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(100);
        let vals = Arc::new((AtomicUsize::new(0), AtomicUsize::new(0)));

        property.subscribe(Arc::new({
            let vals = vals.clone();
            move |old, new| {
                vals.0.store(*old, Ordering::SeqCst);
                vals.1.store(*new, Ordering::SeqCst);
            }
        }))?;

        property.set(200)?;

        assert_eq!(vals.0.load(Ordering::SeqCst), 100);
        assert_eq!(vals.1.load(Ordering::SeqCst), 200);
        Ok(())
    }

    // Test for complex data type
    #[derive(Debug, Clone, PartialEq)]
    struct Person {
        name: String,
        age: u32,
    }

    #[tokio::test]
    async fn test_complex_data_type() -> Result<(), PropertyError> {
        let person1 = Person {
            name: "Alice".to_string(),
            age: 30,
        };

        let person2 = Person {
            name: "Bob".to_string(),
            age: 25,
        };

        let property = ObservableProperty::new(person1.clone());
        assert_eq!(property.get()?, person1);

        let name_changes = Arc::new(AtomicUsize::new(0));

        property.subscribe_filtered(
            Arc::new({
                let counter = name_changes.clone();
                move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
            }),
            |old, new| old.name != new.name // Only notify on name changes
        )?;

        // Update age only - shouldn't trigger
        let mut person3 = person1.clone();
        person3.age = 31;
        property.set(person3)?;
        assert_eq!(name_changes.load(Ordering::SeqCst), 0);

        // Update name - should trigger
        property.set(person2)?;
        assert_eq!(name_changes.load(Ordering::SeqCst), 1);
        Ok(())
    }

    // Test waiting for observers with proper async handling
    #[tokio::test]
    async fn test_waiting_for_observers() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        // Use subscribe_async instead of manually spawning tasks
        let counter_for_observer = counter.clone();
        property.subscribe_async(move |_, _| {
            let counter = counter_for_observer.clone();
            async move {
                sleep(Duration::from_millis(50)).await;
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })?;

        // Use the regular set_async method
        property.set_async(42).await?;

        // Give sufficient time for async observers to complete
        sleep(Duration::from_millis(100)).await;

        // Counter should be incremented after the async work completes
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_subscription_auto_cleanup() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        {
            // Create a subscription in this scope
            let _subscription = property.subscribe_with_token(Arc::new({
                let counter = counter.clone();
                move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
            }))?;

            // Update should trigger the observer
            property.set(1)?;
            assert_eq!(counter.load(Ordering::SeqCst), 1);

            // Subscription is still active within this scope
            property.set(2)?;
            assert_eq!(counter.load(Ordering::SeqCst), 2);
        } // _subscription is dropped here, should automatically unsubscribe

        // After subscription is dropped, updates should not trigger the observer
        property.set(3)?;
        assert_eq!(counter.load(Ordering::SeqCst), 2); // Counter should not increment

        Ok(())
    }

    #[tokio::test]
    async fn test_filtered_subscription_auto_cleanup() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        // Only notify when value increases
        let filter = |old: &i32, new: &i32| new > old;

        {
            // Create a filtered subscription in this scope
            let _subscription = property.subscribe_filtered_with_token(
                Arc::new({
                    let counter = counter.clone();
                    move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
                }),
                filter
            )?;

            property.set(10)?; // Should trigger (0 -> 10)
            assert_eq!(counter.load(Ordering::SeqCst), 1);

            property.set(5)?; // Should NOT trigger (10 -> 5)
            assert_eq!(counter.load(Ordering::SeqCst), 1);

            property.set(15)?; // Should trigger (5 -> 15)
            assert_eq!(counter.load(Ordering::SeqCst), 2);
        } // _subscription is dropped here, should automatically unsubscribe

        // After subscription is dropped, updates should not trigger the observer
        property.set(20)?;
        assert_eq!(counter.load(Ordering::SeqCst), 2); // Counter should not increment

        Ok(())
    }

    #[tokio::test]
    async fn test_async_subscription_auto_cleanup() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone(); // Clone before passing to closure

        {
            // Create an async subscription in this scope
            let _subscription = property.subscribe_async_with_token(move |_, _| {
                let counter = counter_clone.clone(); // Use counter_clone instead of counter
                async move {
                    sleep(Duration::from_millis(10)).await;
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })?;

            property.set_async(1).await?;

            // Give time for async operations to complete
            sleep(Duration::from_millis(50)).await;
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        } // _subscription is dropped here, should automatically unsubscribe

        // After subscription is dropped, updates should not trigger the observer
        property.set_async(2).await?;

        // Give time for any potential async operations to complete
        sleep(Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1); // Counter should not increment

        Ok(())
    }

    #[tokio::test]
    async fn test_async_filtered_subscription_auto_cleanup() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone(); // Clone before passing to closure

        {
            // Create an async filtered subscription in this scope
            let _subscription = property.subscribe_async_filtered_with_token(
                move |_, _| {
                    let counter = counter_clone.clone(); // Use counter_clone instead of counter
                    async move {
                        sleep(Duration::from_millis(10)).await;
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                },
                |old, new| new > old // Only trigger when value increases
            )?;

            property.set_async(10).await?; // Should trigger (0 -> 10)

            // Give time for async operations to complete
            sleep(Duration::from_millis(50)).await;
            assert_eq!(counter.load(Ordering::SeqCst), 1);

            property.set_async(5).await?; // Should NOT trigger (10 -> 5)
            sleep(Duration::from_millis(50)).await;
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        } // _subscription is dropped here, should automatically unsubscribe

        // After subscription is dropped, updates should not trigger the observer
        property.set_async(15).await?; // Would have triggered with active subscription

        // Give time for any potential async operations to complete
        sleep(Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1); // Counter should not increment

        Ok(())
    }

    #[tokio::test]
    async fn test_multiple_subscriptions() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));

        // First subscription
        let subscription1 = property.subscribe_with_token(Arc::new({
            let counter = counter1.clone();
            move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
        }))?;

        // Second subscription
        let subscription2 = property.subscribe_with_token(Arc::new({
            let counter = counter2.clone();
            move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
        }))?;

        // Both subscriptions should receive updates
        property.set(1)?;
        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 1);

        // Drop first subscription only
        drop(subscription1);

        // Only the second subscription should receive updates now
        property.set(2)?;
        assert_eq!(counter1.load(Ordering::SeqCst), 1); // Should not increment
        assert_eq!(counter2.load(Ordering::SeqCst), 2); // Should increment

        // Drop second subscription
        drop(subscription2);

        // No subscriptions should receive updates now
        property.set(3)?;
        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_subscription_with_property_drop() -> Result<(), PropertyError> {
        // Create property in a scope so it can be dropped
        let counter = Arc::new(AtomicUsize::new(0));
        let subscription;

        {
            let property = ObservableProperty::new(0);

            // Create subscription
            subscription = property.subscribe_with_token(Arc::new({
                let counter = counter.clone();
                move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
            }))?;

            // Subscription works normally
            property.set(1)?;
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        } // property is dropped here, but subscription is still alive

        // Subscription should be aware that property is gone when we drop it
        // This should not panic or cause any issues
        drop(subscription);

        Ok(())
    }

    // Test cleanup methods
    #[tokio::test]
    async fn test_cleanup_methods() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(42);
        let counter = Arc::new(AtomicUsize::new(0));

        // Subscribe multiple observers
        let counter1 = counter.clone();
        property.subscribe(Arc::new(move |_, _| {
            counter1.fetch_add(1, Ordering::SeqCst);
        }))?;

        let counter2 = counter.clone();
        property.subscribe_async(move |_, _| {
            let counter = counter2.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })?;

        assert_eq!(property.observer_count(), 2);

        // Test clear_observers
        property.clear_observers()?;
        assert_eq!(property.observer_count(), 0);

        // Setting value should not trigger any observers
        property.set(100)?;
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Re-subscribe to test shutdown
        let counter3 = counter.clone();
        property.subscribe(Arc::new(move |_, _| {
            counter3.fetch_add(1, Ordering::SeqCst);
        }))?;

        assert_eq!(property.observer_count(), 1);

        // Test shutdown method
        property.shutdown()?;
        assert_eq!(property.observer_count(), 0);

        // Setting value should not trigger any observers after shutdown
        property.set(200)?;
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        Ok(())
    }
}

impl From<JoinError> for PropertyError {
    /// Convert a Tokio JoinError into a PropertyError
    ///
    /// This enables using the `?` operator directly on `task::spawn(...).await`
    /// which returns a `Result<T, JoinError>`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    /// use tokio::task;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = Arc::new(ObservableProperty::new(0));
    ///     let property_clone = property.clone();
    ///
    ///     // This task::spawn can now use ?? to propagate both types of errors
    ///     task::spawn(async move {
    ///         property_clone.set(42)?;
    ///         Ok::<_, observable_property_tokio::PropertyError>(())
    ///     }).await??;
    ///
    ///     Ok(())
    /// }
    /// ```
    fn from(err: JoinError) -> Self {
        PropertyError::JoinError(err.to_string())
    }
}
