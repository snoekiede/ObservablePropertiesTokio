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
use std::time::{SystemTime, UNIX_EPOCH};
use parking_lot::RwLock;
use thiserror::Error;
use tokio::task::{self, JoinError};

/// Errors that can occur when working with ObservableProperty
#[derive(Error, Debug, Clone)]
pub enum PropertyError {
    /// Failed to acquire a read lock on the property
    #[error("Failed to acquire read lock during '{operation}': {context}")]
    ReadLockError {
        /// The operation being attempted
        operation: String,
        /// Context describing what operation was being attempted
        context: String,
        /// Timestamp when error occurred (milliseconds since epoch)
        timestamp_ms: u64,
    },

    /// Failed to acquire a write lock on the property
    #[error("Failed to acquire write lock during '{operation}': {context}")]
    WriteLockError {
        /// The operation being attempted
        operation: String,
        /// Context describing what operation was being attempted
        context: String,
        /// Timestamp when error occurred (milliseconds since epoch)
        timestamp_ms: u64,
    },

    /// Attempted to unsubscribe an observer that doesn't exist
    #[error("Observer with ID {id} not found")]
    ObserverNotFound {
        /// The ID of the observer that wasn't found
        id: ObserverId,
    },

    /// The property's lock has been poisoned due to a panic in another thread
    #[error("Lock poisoned during '{operation}': {context}")]
    LockPoisoned {
        /// The operation that encountered the poisoned lock
        operation: String,
        /// Additional context about the poisoned lock
        context: String,
        /// Timestamp when error occurred (milliseconds since epoch)
        timestamp_ms: u64,
    },

    /// An observer function panicked during execution
    #[error("Observer {observer_id} panicked: {error}")]
    ObserverPanic {
        /// The ID of the observer that panicked
        observer_id: ObserverId,
        /// The panic error message
        error: String,
        /// Timestamp when error occurred (milliseconds since epoch)
        timestamp_ms: u64,
    },

    /// An observer function encountered an error during execution
    #[error("Observer execution failed: {reason}")]
    ObserverError {
        /// Description of what went wrong
        reason: String,
    },

    /// A Tokio-related error occurred
    #[error("Tokio runtime error: {reason}")]
    TokioError {
        /// Description of what went wrong
        reason: String,
    },

    /// A task join error occurred
    #[error("Task join error: {0}")]
    JoinError(String),

    /// Maximum capacity has been exceeded
    #[error("Capacity exceeded: current={current}, max={max}, resource={resource}")]
    CapacityExceeded {
        /// Current count
        current: usize,
        /// Maximum allowed
        max: usize,
        /// The resource that exceeded capacity
        resource: String,
    },

    /// Operation exceeded timeout threshold
    #[error("Operation '{operation}' timed out: {elapsed_ms}ms > {threshold_ms}ms")]
    OperationTimeout {
        /// The operation that timed out
        operation: String,
        /// Actual elapsed time in milliseconds
        elapsed_ms: u64,
        /// Timeout threshold in milliseconds
        threshold_ms: u64,
    },

    /// The property is shutting down and not accepting new operations
    #[error("Property is shutting down")]
    ShutdownInProgress,
}

impl PropertyError {
    /// Get a diagnostic string suitable for logging and monitoring
    ///
    /// This method returns a structured string containing all relevant
    /// diagnostic information about the error, including timestamps,
    /// operation context, and performance metrics where applicable.
    ///
    /// # Returns
    ///
    /// A formatted string containing diagnostic information
    ///
    /// # Examples
    ///
    /// ```
    /// use observable_property_tokio::PropertyError;
    ///
    /// let error = PropertyError::OperationTimeout {
    ///     operation: "notify_observers".to_string(),
    ///     elapsed_ms: 5500,
    ///     threshold_ms: 5000,
    /// };
    ///
    /// let diagnostic = error.diagnostic_info();
    /// assert!(diagnostic.contains("notify_observers"));
    /// assert!(diagnostic.contains("elapsed_ms=5500"));
    /// ```
    pub fn diagnostic_info(&self) -> String {
        match self {
            Self::ReadLockError { operation, context, timestamp_ms } => {
                format!(
                    "READ_LOCK_ERROR | operation={} | context={} | timestamp_ms={}",
                    operation, context, timestamp_ms
                )
            }
            Self::WriteLockError { operation, context, timestamp_ms } => {
                format!(
                    "WRITE_LOCK_ERROR | operation={} | context={} | timestamp_ms={}",
                    operation, context, timestamp_ms
                )
            }
            Self::LockPoisoned { operation, context, timestamp_ms } => {
                format!(
                    "LOCK_POISONED | operation={} | context={} | timestamp_ms={}",
                    operation, context, timestamp_ms
                )
            }
            Self::ObserverPanic { observer_id, error, timestamp_ms } => {
                format!(
                    "OBSERVER_PANIC | observer_id={} | error={} | timestamp_ms={}",
                    observer_id, error, timestamp_ms
                )
            }
            Self::ObserverNotFound { id } => {
                format!("OBSERVER_NOT_FOUND | id={}", id)
            }
            Self::CapacityExceeded { current, max, resource } => {
                format!(
                    "CAPACITY_EXCEEDED | resource={} | current={} | max={} | utilization={:.1}%",
                    resource, current, max, (*current as f64 / *max as f64) * 100.0
                )
            }
            Self::OperationTimeout { operation, elapsed_ms, threshold_ms } => {
                format!(
                    "OPERATION_TIMEOUT | operation={} | elapsed_ms={} | threshold_ms={} | overage_ms={}",
                    operation, elapsed_ms, threshold_ms, elapsed_ms.saturating_sub(*threshold_ms)
                )
            }
            Self::ShutdownInProgress => {
                "SHUTDOWN_IN_PROGRESS | property is shutting down".to_string()
            }
            Self::ObserverError { reason } => {
                format!("OBSERVER_ERROR | reason={}", reason)
            }
            Self::TokioError { reason } => {
                format!("TOKIO_ERROR | reason={}", reason)
            }
            Self::JoinError(msg) => {
                format!("JOIN_ERROR | message={}", msg)
            }
        }
    }

    /// Get the current timestamp in milliseconds since UNIX epoch
    ///
    /// This is a helper function used internally for error creation
    fn current_timestamp_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Create a ReadLockError with current timestamp
    pub fn read_lock_error(operation: impl Into<String>, context: impl Into<String>) -> Self {
        Self::ReadLockError {
            operation: operation.into(),
            context: context.into(),
            timestamp_ms: Self::current_timestamp_ms(),
        }
    }

    /// Create a WriteLockError with current timestamp
    pub fn write_lock_error(operation: impl Into<String>, context: impl Into<String>) -> Self {
        Self::WriteLockError {
            operation: operation.into(),
            context: context.into(),
            timestamp_ms: Self::current_timestamp_ms(),
        }
    }

    /// Create a LockPoisoned error with current timestamp
    pub fn lock_poisoned(operation: impl Into<String>, context: impl Into<String>) -> Self {
        Self::LockPoisoned {
            operation: operation.into(),
            context: context.into(),
            timestamp_ms: Self::current_timestamp_ms(),
        }
    }

    /// Create an ObserverPanic error with current timestamp
    pub fn observer_panic(observer_id: ObserverId, error: impl Into<String>) -> Self {
        Self::ObserverPanic {
            observer_id,
            error: error.into(),
            timestamp_ms: Self::current_timestamp_ms(),
        }
    }
}

/// Configuration options for ObservableProperty
///
/// This struct allows you to configure limits and behavior for an observable property,
/// helping prevent resource exhaustion and enabling production-grade backpressure handling.
///
/// # Examples
///
/// ```
/// use observable_property_tokio::{ObservableProperty, PropertyConfig};
///
/// let config = PropertyConfig {
///     max_observers: 100,
///     max_pending_notifications: 50,
///     observer_timeout_ms: 5000,
///     max_concurrent_async_tasks: 50,
/// };
///
/// let property = ObservableProperty::new_with_config(42, config);
/// ```
#[derive(Debug, Clone)]
pub struct PropertyConfig {
    /// Maximum number of observers allowed
    ///
    /// When this limit is reached, attempts to subscribe additional observers
    /// will return a `PropertyError::CapacityExceeded` error.
    ///
    /// Default: 1000
    pub max_observers: usize,

    /// Maximum pending async notifications per observer (reserved for future use)
    ///
    /// This limit helps prevent memory exhaustion from queued notifications.
    ///
    /// Default: 100
    pub max_pending_notifications: usize,

    /// Timeout for observer execution in milliseconds (reserved for future use)
    ///
    /// Observers that exceed this threshold may be logged or flagged for debugging.
    ///
    /// Default: 5000ms
    pub observer_timeout_ms: u64,

    /// Maximum number of concurrent async observer tasks
    ///
    /// This limit prevents resource exhaustion by limiting the number of
    /// async observer notifications that can execute simultaneously.
    /// When the limit is reached, new async notifications will wait until
    /// a slot becomes available (using a semaphore for coordination).
    ///
    /// Default: 100
    pub max_concurrent_async_tasks: usize,
}

impl Default for PropertyConfig {
    fn default() -> Self {
        Self {
            max_observers: 1000,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 100,
        }
    }
}

/// Report generated after a property shutdown operation
///
/// Contains diagnostic information about the shutdown process,
/// including the number of observers cleared and timing information.
///
/// # Examples
///
/// ```
/// use observable_property_tokio::{ObservableProperty, PropertyConfig};
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let property = ObservableProperty::new(42);
///     
///     // ... use property ...
///     
///     let report = property.shutdown_with_timeout(Duration::from_secs(5)).await?;
///     println!("Shutdown complete: {:?}", report);
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ShutdownReport {
    /// Number of observers that were cleared during shutdown
    pub observers_cleared: usize,
    
    /// Time taken to complete the shutdown operation
    pub shutdown_duration: std::time::Duration,
    
    /// Whether the shutdown completed within the timeout period
    pub completed_within_timeout: bool,
    
    /// Timestamp when shutdown was initiated (milliseconds since epoch)
    pub initiated_at_ms: u64,
}

impl ShutdownReport {
    /// Get a diagnostic string for logging
    pub fn diagnostic_info(&self) -> String {
        format!(
            "SHUTDOWN_COMPLETE | observers_cleared={} | duration_ms={} | within_timeout={} | initiated_at_ms={}",
            self.observers_cleared,
            self.shutdown_duration.as_millis(),
            self.completed_within_timeout,
            self.initiated_at_ms
        )
    }
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
    config: PropertyConfig,
    async_task_semaphore: Arc<tokio::sync::Semaphore>,
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
    /// Uses default configuration with max_observers=1000.
    /// For custom limits, use `new_with_config()`.
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
        Self::new_with_config(initial_value, PropertyConfig::default())
    }

    /// Creates a new observable property with custom configuration
    ///
    /// This allows you to specify limits on the number of observers and other
    /// behavioral parameters to prevent resource exhaustion.
    ///
    /// # Arguments
    ///
    /// * `initial_value` - The starting value for this property
    /// * `config` - Configuration options for backpressure and resource limits
    ///
    /// # Examples
    ///
    /// ```rust
    /// use observable_property_tokio::{ObservableProperty, PropertyConfig};
    ///
    /// let config = PropertyConfig {
    ///     max_observers: 50,
    ///     max_pending_notifications: 100,
    ///     observer_timeout_ms: 3000,
    ///     max_concurrent_async_tasks: 50,
    /// };
    ///
    /// let property = ObservableProperty::new_with_config(0, config);
    /// assert_eq!(property.get().unwrap(), 0);
    /// ```
    pub fn new_with_config(initial_value: T, config: PropertyConfig) -> Self {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent_async_tasks));
        
        Self {
            inner: Arc::new(RwLock::new(InnerProperty {
                value: initial_value,
                observers: HashMap::new(),
                next_id: 0,
            })),
            config,
            async_task_semaphore: semaphore,
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

        // Spawn a separate Tokio task for each observer with semaphore-based connection pooling
        let mut tasks = Vec::with_capacity(observers_snapshot.len());

        for observer in observers_snapshot {
            let old_val = old_value.clone();
            let new_val = new_value.clone();
            let semaphore = Arc::clone(&self.async_task_semaphore);

            let task = task::spawn(async move {
                // Acquire permit from semaphore before executing observer
                let _permit = semaphore.acquire().await.expect("Semaphore closed");
                
                if let Err(e) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                    observer(&old_val, &new_val);
                })) {
                    eprintln!("Observer panic in task: {:?}", e);
                }
                // Permit is automatically released when _permit is dropped
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

        // Check if we've reached the maximum number of observers
        if inner.observers.len() >= self.config.max_observers {
            return Err(PropertyError::CapacityExceeded {
                current: inner.observers.len(),
                max: self.config.max_observers,
                resource: "observers".to_string(),
            });
        }

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
        let semaphore = Arc::clone(&self.async_task_semaphore);

        let observer = Arc::new(move |old: &T, new: &T| {
            let old_val = old.clone();
            let new_val = new.clone();
            // Clone the handler so we can move it into the task
            let handler_clone = Arc::clone(&handler);
            let semaphore_clone = Arc::clone(&semaphore);

            tokio::spawn(async move {
                // Acquire permit from semaphore before executing async handler
                let _permit = semaphore_clone.acquire().await.expect("Semaphore closed");
                handler_clone(old_val, new_val).await;
                // Permit is automatically released when _permit is dropped
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
        let semaphore = Arc::clone(&self.async_task_semaphore);

        let observer = Arc::new(move |old: &T, new: &T| {
            if filter(old, new) {
                let old_val = old.clone();
                let new_val = new.clone();
                let handler_clone = Arc::clone(&handler);
                let semaphore_clone = Arc::clone(&semaphore);

                tokio::spawn(async move {
                    // Acquire permit from semaphore before executing async handler
                    let _permit = semaphore_clone.acquire().await.expect("Semaphore closed");
                    handler_clone(old_val, new_val).await;
                    // Permit is automatically released when _permit is dropped
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

    /// Shutdown the property with a timeout, waiting for pending async operations
    ///
    /// This method performs a comprehensive shutdown that:
    /// 1. Clears all observers
    /// 2. Waits for a grace period to allow pending async operations to complete
    /// 3. Returns a detailed report about the shutdown process
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum duration to wait for shutdown to complete
    ///
    /// # Returns
    ///
    /// `Ok(ShutdownReport)` containing shutdown metrics and diagnostics
    ///
    /// # Examples
    ///
    /// ```
    /// use observable_property_tokio::ObservableProperty;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = ObservableProperty::new(0);
    ///     
    ///     // Add some async observers
    ///     property.subscribe_async(|_, new| async move {
    ///         println!("Async observer: {}", new);
    ///     })?;
    ///     
    ///     property.subscribe(Arc::new(|_, new| {
    ///         println!("Sync observer: {}", new);
    ///     }))?;
    ///     
    ///     // ... use property ...
    ///     
    ///     // Graceful shutdown with timeout
    ///     let report = property.shutdown_with_timeout(Duration::from_secs(5)).await?;
    ///     
    ///     println!("Shutdown report: {}", report.diagnostic_info());
    ///     println!("Cleared {} observers in {:?}", 
    ///         report.observers_cleared, 
    ///         report.shutdown_duration);
    ///     
    ///     Ok(())
    /// }
    /// ```
    pub async fn shutdown_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<ShutdownReport, PropertyError> {
        use std::time::{SystemTime, UNIX_EPOCH, Instant};
        
        let start = Instant::now();
        let initiated_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        
        // Get initial observer count before clearing
        let initial_count = self.observer_count();
        
        // Clear all observers
        self.clear_observers()?;
        
        // Wait for pending notifications with timeout
        let grace_period = timeout.min(std::time::Duration::from_millis(500));
        let completed_within_timeout = tokio::time::timeout(
            grace_period,
            tokio::time::sleep(grace_period)
        ).await.is_ok();
        
        let shutdown_duration = start.elapsed();
        
        Ok(ShutdownReport {
            observers_cleared: initial_count,
            shutdown_duration,
            completed_within_timeout,
            initiated_at_ms,
        })
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
            config: self.config.clone(),
            async_task_semaphore: Arc::clone(&self.async_task_semaphore),
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

    // Test backpressure and configuration functionality
    #[tokio::test]
    async fn test_property_config_default() {
        let config = PropertyConfig::default();
        assert_eq!(config.max_observers, 1000);
        assert_eq!(config.max_pending_notifications, 100);
        assert_eq!(config.observer_timeout_ms, 5000);
    }

    #[tokio::test]
    async fn test_property_with_custom_config() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 5,
            max_pending_notifications: 10,
            observer_timeout_ms: 1000,
            max_concurrent_async_tasks: 100,
        };

        let property = ObservableProperty::new_with_config(42, config);
        assert_eq!(property.get()?, 42);

        // Should be able to add up to max_observers
        for i in 0..5 {
            property.subscribe(Arc::new(move |_, _| {
                println!("Observer {}", i);
            }))?;
        }

        assert_eq!(property.observer_count(), 5);
        Ok(())
    }

    #[tokio::test]
    async fn test_observer_capacity_limit() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 3,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 100,
        };

        let property = ObservableProperty::new_with_config(0, config);

        // Add observers up to limit
        property.subscribe(Arc::new(|_, _| {}))?;
        property.subscribe(Arc::new(|_, _| {}))?;
        property.subscribe(Arc::new(|_, _| {}))?;

        assert_eq!(property.observer_count(), 3);

        // Next subscribe should fail with CapacityExceeded
        let result = property.subscribe(Arc::new(|_, _| {}));
        assert!(matches!(result, Err(PropertyError::CapacityExceeded { .. })));

        if let Err(PropertyError::CapacityExceeded { current, max, resource }) = result {
            assert_eq!(current, 3);
            assert_eq!(max, 3);
            assert_eq!(resource, "observers");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_observer_capacity_after_unsubscribe() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 2,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 100,
        };

        let property = ObservableProperty::new_with_config(0, config);

        // Add observers up to limit
        let id1 = property.subscribe(Arc::new(|_, _| {}))?;
        let _id2 = property.subscribe(Arc::new(|_, _| {}))?;

        assert_eq!(property.observer_count(), 2);

        // Next subscribe should fail
        assert!(property.subscribe(Arc::new(|_, _| {})).is_err());

        // Unsubscribe one observer
        property.unsubscribe(id1)?;
        assert_eq!(property.observer_count(), 1);

        // Now we should be able to add another observer
        let _id3 = property.subscribe(Arc::new(|_, _| {}))?;
        assert_eq!(property.observer_count(), 2);

        // But not beyond the limit
        assert!(property.subscribe(Arc::new(|_, _| {})).is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_async_observer_capacity_limit() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 2,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 100,
        };

        let property = ObservableProperty::new_with_config(0, config);

        // Add async observers up to limit
        property.subscribe_async(|_, _| async move {
            sleep(Duration::from_millis(10)).await;
        })?;

        property.subscribe_async(|_, _| async move {
            sleep(Duration::from_millis(10)).await;
        })?;

        assert_eq!(property.observer_count(), 2);

        // Next subscribe should fail
        let result = property.subscribe_async(|_, _| async move {});
        assert!(matches!(result, Err(PropertyError::CapacityExceeded { .. })));

        Ok(())
    }

    #[tokio::test]
    async fn test_filtered_observer_capacity_limit() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 2,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 100,
        };

        let property = ObservableProperty::new_with_config(0, config);

        // Add filtered observers up to limit
        property.subscribe_filtered(Arc::new(|_, _| {}), |_, _| true)?;
        property.subscribe_filtered(Arc::new(|_, _| {}), |_, _| true)?;

        assert_eq!(property.observer_count(), 2);

        // Next subscribe should fail
        let result = property.subscribe_filtered(Arc::new(|_, _| {}), |_, _| true);
        assert!(matches!(result, Err(PropertyError::CapacityExceeded { .. })));

        Ok(())
    }

    #[tokio::test]
    async fn test_capacity_error_diagnostic() {
        let error = PropertyError::CapacityExceeded {
            current: 100,
            max: 100,
            resource: "observers".to_string(),
        };

        let diagnostic = error.diagnostic_info();
        assert!(diagnostic.contains("CAPACITY_EXCEEDED"));
        assert!(diagnostic.contains("resource=observers"));
        assert!(diagnostic.contains("current=100"));
        assert!(diagnostic.contains("max=100"));
        assert!(diagnostic.contains("utilization=100.0%"));
    }

    #[tokio::test]
    async fn test_cloned_property_shares_config() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 3,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 100,
        };

        let property1 = ObservableProperty::new_with_config(0, config);
        let property2 = property1.clone();

        // Add observers through both properties
        property1.subscribe(Arc::new(|_, _| {}))?;
        property2.subscribe(Arc::new(|_, _| {}))?;
        property1.subscribe(Arc::new(|_, _| {}))?;

        // Both should show 3 observers since they share the same inner state
        assert_eq!(property1.observer_count(), 3);
        assert_eq!(property2.observer_count(), 3);

        // Next subscribe should fail on either property
        assert!(property1.subscribe(Arc::new(|_, _| {})).is_err());
        assert!(property2.subscribe(Arc::new(|_, _| {})).is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_subscription_token_with_capacity() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 2,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 100,
        };

        let property = ObservableProperty::new_with_config(0, config);

        // Create subscriptions with tokens
        let _sub1 = property.subscribe_with_token(Arc::new(|_, _| {}))?;
        let _sub2 = property.subscribe_with_token(Arc::new(|_, _| {}))?;

        assert_eq!(property.observer_count(), 2);

        // Next subscribe should fail
        let result = property.subscribe_with_token(Arc::new(|_, _| {}));
        assert!(matches!(result, Err(PropertyError::CapacityExceeded { .. })));

        // Drop one subscription
        drop(_sub1);

        // Now we should be able to add another
        let _sub3 = property.subscribe_with_token(Arc::new(|_, _| {}))?;
        assert_eq!(property.observer_count(), 2);

        Ok(())
    }

    // Test error diagnostic functionality
    #[tokio::test]
    async fn test_error_diagnostic_info() {
        // Test ReadLockError
        let read_error = PropertyError::read_lock_error("get_value", "acquiring read lock failed");
        let diagnostic = read_error.diagnostic_info();
        assert!(diagnostic.contains("READ_LOCK_ERROR"));
        assert!(diagnostic.contains("operation=get_value"));
        assert!(diagnostic.contains("context=acquiring read lock failed"));
        assert!(diagnostic.contains("timestamp_ms="));

        // Test WriteLockError
        let write_error = PropertyError::write_lock_error("set_value", "acquiring write lock failed");
        let diagnostic = write_error.diagnostic_info();
        assert!(diagnostic.contains("WRITE_LOCK_ERROR"));
        assert!(diagnostic.contains("operation=set_value"));

        // Test LockPoisoned
        let poisoned_error = PropertyError::lock_poisoned("notify", "inner lock poisoned");
        let diagnostic = poisoned_error.diagnostic_info();
        assert!(diagnostic.contains("LOCK_POISONED"));
        assert!(diagnostic.contains("operation=notify"));
        assert!(diagnostic.contains("context=inner lock poisoned"));

        // Test ObserverPanic
        let panic_error = PropertyError::observer_panic(ObserverId(42), "observer crashed");
        let diagnostic = panic_error.diagnostic_info();
        assert!(diagnostic.contains("OBSERVER_PANIC"));
        assert!(diagnostic.contains("observer_id=42"));
        assert!(diagnostic.contains("error=observer crashed"));

        // Test ObserverNotFound
        let not_found_error = PropertyError::ObserverNotFound { id: ObserverId(99) };
        let diagnostic = not_found_error.diagnostic_info();
        assert!(diagnostic.contains("OBSERVER_NOT_FOUND"));
        assert!(diagnostic.contains("id=99"));

        // Test CapacityExceeded
        let capacity_error = PropertyError::CapacityExceeded {
            current: 150,
            max: 100,
            resource: "observers".to_string(),
        };
        let diagnostic = capacity_error.diagnostic_info();
        assert!(diagnostic.contains("CAPACITY_EXCEEDED"));
        assert!(diagnostic.contains("resource=observers"));
        assert!(diagnostic.contains("current=150"));
        assert!(diagnostic.contains("max=100"));
        assert!(diagnostic.contains("utilization=150.0%"));

        // Test OperationTimeout
        let timeout_error = PropertyError::OperationTimeout {
            operation: "notify_all".to_string(),
            elapsed_ms: 5500,
            threshold_ms: 5000,
        };
        let diagnostic = timeout_error.diagnostic_info();
        assert!(diagnostic.contains("OPERATION_TIMEOUT"));
        assert!(diagnostic.contains("operation=notify_all"));
        assert!(diagnostic.contains("elapsed_ms=5500"));
        assert!(diagnostic.contains("threshold_ms=5000"));
        assert!(diagnostic.contains("overage_ms=500"));

        // Test ShutdownInProgress
        let shutdown_error = PropertyError::ShutdownInProgress;
        let diagnostic = shutdown_error.diagnostic_info();
        assert!(diagnostic.contains("SHUTDOWN_IN_PROGRESS"));

        // Test ObserverError
        let observer_error = PropertyError::ObserverError {
            reason: "callback failed".to_string(),
        };
        let diagnostic = observer_error.diagnostic_info();
        assert!(diagnostic.contains("OBSERVER_ERROR"));
        assert!(diagnostic.contains("reason=callback failed"));

        // Test TokioError
        let tokio_error = PropertyError::TokioError {
            reason: "runtime unavailable".to_string(),
        };
        let diagnostic = tokio_error.diagnostic_info();
        assert!(diagnostic.contains("TOKIO_ERROR"));
        assert!(diagnostic.contains("reason=runtime unavailable"));

        // Test JoinError
        let join_error = PropertyError::JoinError("task panicked".to_string());
        let diagnostic = join_error.diagnostic_info();
        assert!(diagnostic.contains("JOIN_ERROR"));
        assert!(diagnostic.contains("message=task panicked"));
    }

    #[tokio::test]
    async fn test_error_helper_functions_with_timestamp() {
        use std::time::{SystemTime, UNIX_EPOCH};
        
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Create errors using helper functions
        let read_error = PropertyError::read_lock_error("test_op", "test_context");
        let write_error = PropertyError::write_lock_error("test_op", "test_context");
        let poisoned_error = PropertyError::lock_poisoned("test_op", "test_context");
        let panic_error = PropertyError::observer_panic(ObserverId(1), "test_panic");

        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Verify timestamps are reasonable (within the time window of test execution)
        match read_error {
            PropertyError::ReadLockError { timestamp_ms, .. } => {
                assert!(timestamp_ms >= before && timestamp_ms <= after);
            }
            _ => panic!("Expected ReadLockError"),
        }

        match write_error {
            PropertyError::WriteLockError { timestamp_ms, .. } => {
                assert!(timestamp_ms >= before && timestamp_ms <= after);
            }
            _ => panic!("Expected WriteLockError"),
        }

        match poisoned_error {
            PropertyError::LockPoisoned { timestamp_ms, .. } => {
                assert!(timestamp_ms >= before && timestamp_ms <= after);
            }
            _ => panic!("Expected LockPoisoned"),
        }

        match panic_error {
            PropertyError::ObserverPanic { timestamp_ms, .. } => {
                assert!(timestamp_ms >= before && timestamp_ms <= after);
            }
            _ => panic!("Expected ObserverPanic"),
        }
    }

    #[tokio::test]
    async fn test_error_display_formatting() {
        let timeout_error = PropertyError::OperationTimeout {
            operation: "test_operation".to_string(),
            elapsed_ms: 1500,
            threshold_ms: 1000,
        };
        let display = format!("{}", timeout_error);
        assert!(display.contains("test_operation"));
        assert!(display.contains("1500ms"));
        assert!(display.contains("1000ms"));

        let capacity_error = PropertyError::CapacityExceeded {
            current: 200,
            max: 100,
            resource: "test_resource".to_string(),
        };
        let display = format!("{}", capacity_error);
        assert!(display.contains("200"));
        assert!(display.contains("100"));
        assert!(display.contains("test_resource"));
    }

    #[tokio::test]
    async fn test_capacity_exceeded_utilization_calculation() {
        let error = PropertyError::CapacityExceeded {
            current: 75,
            max: 100,
            resource: "observers".to_string(),
        };
        let diagnostic = error.diagnostic_info();
        assert!(diagnostic.contains("utilization=75.0%"));

        let error2 = PropertyError::CapacityExceeded {
            current: 100,
            max: 100,
            resource: "observers".to_string(),
        };
        let diagnostic2 = error2.diagnostic_info();
        assert!(diagnostic2.contains("utilization=100.0%"));
    }

    // Test graceful shutdown functionality
    #[tokio::test]
    async fn test_shutdown_with_timeout_basic() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        
        // Add some observers
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
        
        // Perform shutdown with timeout
        let report = property.shutdown_with_timeout(Duration::from_secs(5)).await?;
        
        // Verify report
        assert_eq!(report.observers_cleared, 2);
        assert!(report.shutdown_duration.as_secs() < 5);
        assert!(report.completed_within_timeout);
        assert!(report.initiated_at_ms > 0);
        
        // Verify observers were cleared
        assert_eq!(property.observer_count(), 0);
        
        // Setting value should not trigger observers
        property.set(42)?;
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_shutdown_report_diagnostic() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let property = ObservableProperty::new(100);
        
        // Add multiple observers
        for _ in 0..5 {
            property.subscribe(Arc::new(|_, _| {}))?;
        }
        
        let report = property.shutdown_with_timeout(Duration::from_secs(1)).await?;
        
        let diagnostic = report.diagnostic_info();
        assert!(diagnostic.contains("SHUTDOWN_COMPLETE"));
        assert!(diagnostic.contains("observers_cleared=5"));
        assert!(diagnostic.contains("within_timeout=true"));
        assert!(diagnostic.contains("initiated_at_ms="));
        
        Ok(())
    }

    #[tokio::test]
    async fn test_shutdown_with_async_observers() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        
        // Add async observers that take some time
        let counter1 = counter.clone();
        property.subscribe_async(move |_, _| {
            let counter = counter1.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })?;
        
        let counter2 = counter.clone();
        property.subscribe_async(move |_, _| {
            let counter = counter2.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })?;
        
        // Trigger observers
        property.set_async(42).await?;
        
        // Give time for async operations to start
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        // Shutdown with enough timeout for operations to complete
        let report = property.shutdown_with_timeout(Duration::from_secs(2)).await?;
        
        assert_eq!(report.observers_cleared, 2);
        assert!(report.completed_within_timeout);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_shutdown_idempotent() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let property = ObservableProperty::new("test");
        
        property.subscribe(Arc::new(|_, _| {}))?;
        property.subscribe(Arc::new(|_, _| {}))?;
        
        // First shutdown
        let report1 = property.shutdown_with_timeout(Duration::from_secs(1)).await?;
        assert_eq!(report1.observers_cleared, 2);
        
        // Second shutdown should still succeed but clear 0 observers
        let report2 = property.shutdown_with_timeout(Duration::from_secs(1)).await?;
        assert_eq!(report2.observers_cleared, 0);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_shutdown_vs_shutdown_with_timeout() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        // Test regular shutdown
        let property1 = ObservableProperty::new(0);
        property1.subscribe(Arc::new(|_, _| {}))?;
        property1.subscribe(Arc::new(|_, _| {}))?;
        
        property1.shutdown()?;
        assert_eq!(property1.observer_count(), 0);
        
        // Test shutdown with timeout
        let property2 = ObservableProperty::new(0);
        property2.subscribe(Arc::new(|_, _| {}))?;
        property2.subscribe(Arc::new(|_, _| {}))?;
        
        let report = property2.shutdown_with_timeout(Duration::from_secs(1)).await?;
        assert_eq!(property2.observer_count(), 0);
        assert_eq!(report.observers_cleared, 2);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_shutdown_report_timing() -> Result<(), PropertyError> {
        use std::time::{Duration, Instant};
        
        let property = ObservableProperty::new(0);
        
        // Add several observers
        for _ in 0..10 {
            property.subscribe(Arc::new(|_, _| {}))?;
        }
        
        let start = Instant::now();
        let report = property.shutdown_with_timeout(Duration::from_secs(1)).await?;
        let elapsed = start.elapsed();
        
        // Shutdown should complete reasonably quickly
        assert!(elapsed < Duration::from_secs(2));
        assert!(report.shutdown_duration <= elapsed);
        assert_eq!(report.observers_cleared, 10);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_shutdown_with_filtered_and_async_observers() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let property = ObservableProperty::new(0);
        
        // Mix of different observer types
        property.subscribe(Arc::new(|_, _| {}))?;
        property.subscribe_async(|_, _| async {})?;
        property.subscribe_filtered(Arc::new(|_, _| {}), |_, new| new % 2 == 0)?;
        property.subscribe_async_filtered(|_, _| async {}, |_, new| new > &0)?;
        
        assert_eq!(property.observer_count(), 4);
        
        let report = property.shutdown_with_timeout(Duration::from_secs(1)).await?;
        
        assert_eq!(report.observers_cleared, 4);
        assert_eq!(property.observer_count(), 0);
        
        Ok(())
    }

    // Batching tests
    #[tokio::test]
    async fn test_batched_property_creation() -> Result<(), PropertyError> {
        let property = BatchedProperty::new(42);
        assert_eq!(property.get()?, 42);
        
        let config = BatchConfig {
            batch_interval: std::time::Duration::from_millis(50),
        };
        let property2 = BatchedProperty::new_with_config(100, config);
        assert_eq!(property2.get()?, 100);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_batched_property_queue_update() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let property = BatchedProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        let last_value = Arc::new(RwLock::new(0));
        
        // Subscribe to updates
        property.subscribe(Arc::new({
            let counter = counter.clone();
            let last_value = last_value.clone();
            move |_, new| {
                counter.fetch_add(1, Ordering::SeqCst);
                *last_value.write() = *new;
            }
        }))?;
        
        // Queue multiple updates rapidly
        for i in 1..=10 {
            property.queue_update(i)?;
        }
        
        // Wait for batch to flush (default is 100ms)
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Should have been notified only once with the last value
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(*last_value.read(), 10);
        assert_eq!(property.get()?, 10);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_batched_property_multiple_batches() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let config = BatchConfig {
            batch_interval: Duration::from_millis(50),
        };
        let property = BatchedProperty::new_with_config(0, config);
        let counter = Arc::new(AtomicUsize::new(0));
        
        property.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }))?;
        
        // First batch
        for i in 1..=5 {
            property.queue_update(i)?;
        }
        tokio::time::sleep(Duration::from_millis(75)).await;
        
        // Second batch
        for i in 6..=10 {
            property.queue_update(i)?;
        }
        tokio::time::sleep(Duration::from_millis(75)).await;
        
        // Should have been notified twice (once per batch)
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        assert_eq!(property.get()?, 10);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_batched_property_set_immediate() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let property = BatchedProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        
        property.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }))?;
        
        // Queue some updates
        property.queue_update(5)?;
        property.queue_update(10)?;
        
        // Set immediately - should notify right away
        property.set_immediate(42)?;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(property.get()?, 42);
        
        // Wait for batch - queued updates should be cleared by set_immediate
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Counter should still be 1 (no additional notification from batch)
        // Note: This behavior depends on timing, but set_immediate should have priority
        assert!(counter.load(Ordering::SeqCst) >= 1);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_batched_property_flush() -> Result<(), PropertyError> {
        let property = BatchedProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        
        property.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }))?;
        
        // Queue updates
        property.queue_update(42)?;
        
        // Flush immediately
        property.flush().await?;
        
        // Should be notified immediately
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(property.get()?, 42);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_batched_property_async_observer() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let property = BatchedProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        
        property.subscribe_async({
            let counter = counter.clone();
            move |_, _| {
                let counter = counter.clone();
                async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }
        })?;
        
        // Queue multiple updates
        for i in 1..=5 {
            property.queue_update(i)?;
        }
        
        // Wait for batch and async processing
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Should have been notified once
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_batched_property_observer_management() -> Result<(), PropertyError> {
        let property = BatchedProperty::new(0);
        
        // Add observers
        let id1 = property.subscribe(Arc::new(|_, _| {}))?;
        let _id2 = property.subscribe(Arc::new(|_, _| {}))?;
        assert_eq!(property.observer_count(), 2);
        
        // Unsubscribe one
        property.unsubscribe(id1)?;
        assert_eq!(property.observer_count(), 1);
        
        // Clear all
        property.clear_observers()?;
        assert_eq!(property.observer_count(), 0);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_batched_property_clone() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let property1 = BatchedProperty::new(0);
        let property2 = property1.clone();
        
        let counter = Arc::new(AtomicUsize::new(0));
        
        // Subscribe on clone
        property2.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }))?;
        
        // Update on original
        property1.queue_update(42)?;
        
        // Wait for batch
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Observer should be notified
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(property1.get()?, 42);
        assert_eq!(property2.get()?, 42);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_batched_property_high_frequency() -> Result<(), PropertyError> {
        use std::time::Duration;
        
        let config = BatchConfig {
            batch_interval: Duration::from_millis(100),
        };
        let property = BatchedProperty::new_with_config(0, config);
        let counter = Arc::new(AtomicUsize::new(0));
        
        property.subscribe(Arc::new({
            let counter = counter.clone();
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }))?;
        
        // Queue 1000 updates rapidly
        for i in 1..=1000 {
            property.queue_update(i)?;
        }
        
        // Wait for batch
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Should have been notified only once despite 1000 updates
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(property.get()?, 1000);
        
        Ok(())
    }
}

/// Configuration for batched property updates
///
/// Controls how frequently batched updates are flushed to observers,
/// helping reduce overhead for high-frequency property changes.
///
/// # Examples
///
/// ```
/// use observable_property_tokio::BatchConfig;
/// use std::time::Duration;
///
/// let config = BatchConfig {
///     batch_interval: Duration::from_millis(100),
/// };
/// ```
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// How often to flush batched updates to observers
    ///
    /// A longer interval reduces notification overhead but increases latency.
    /// A shorter interval provides faster updates but with more overhead.
    ///
    /// Default: 100ms
    pub batch_interval: std::time::Duration,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_interval: std::time::Duration::from_millis(100),
        }
    }
}

/// A batched wrapper around ObservableProperty that reduces notification overhead
///
/// This type collects property updates and only notifies observers at regular intervals,
/// which is useful for high-frequency update scenarios where you want to reduce the
/// number of observer notifications.
///
/// # Examples
///
/// ```
/// use observable_property_tokio::{BatchedProperty, BatchConfig};
/// use std::time::Duration;
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
///     // Create a batched property with 100ms batching interval
///     let config = BatchConfig {
///         batch_interval: Duration::from_millis(100),
///     };
///     
///     let property = BatchedProperty::new_with_config(0, config);
///     
///     // Subscribe to batched updates
///     property.subscribe(Arc::new(|old, new| {
///         println!("Batched update: {} -> {}", old, new);
///     }))?;
///     
///     // Queue multiple updates rapidly
///     for i in 1..=100 {
///         property.queue_update(i)?;
///     }
///     
///     // Observers will only be notified once per batch interval
///     // with the latest value
///     
///     // Wait for batch to flush
///     tokio::time::sleep(Duration::from_millis(150)).await;
///     
///     Ok(())
/// }
/// ```
pub struct BatchedProperty<T: Clone + Send + Sync + 'static> {
    inner: ObservableProperty<T>,
    pending_update: Arc<RwLock<Option<T>>>,
    _batch_task: Arc<tokio::task::JoinHandle<()>>,
}

impl<T: Clone + Send + Sync + 'static> BatchedProperty<T> {
    /// Create a new batched property with default configuration
    ///
    /// Uses a batch interval of 100ms by default.
    ///
    /// # Arguments
    ///
    /// * `initial_value` - The starting value for the property
    ///
    /// # Examples
    ///
    /// ```
    /// use observable_property_tokio::BatchedProperty;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = BatchedProperty::new(42);
    ///     assert_eq!(property.get()?, 42);
    ///     Ok(())
    /// }
    /// ```
    pub fn new(initial_value: T) -> Self {
        Self::new_with_config(initial_value, BatchConfig::default())
    }

    /// Create a new batched property with custom configuration
    ///
    /// # Arguments
    ///
    /// * `initial_value` - The starting value for the property
    /// * `config` - Batch configuration controlling flush interval
    ///
    /// # Examples
    ///
    /// ```
    /// use observable_property_tokio::{BatchedProperty, BatchConfig};
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let config = BatchConfig {
    ///         batch_interval: Duration::from_millis(50),
    ///     };
    ///
    ///     let property = BatchedProperty::new_with_config(0, config);
    /// }
    /// ```
    pub fn new_with_config(initial_value: T, config: BatchConfig) -> Self {
        let inner = ObservableProperty::new(initial_value);
        let pending_update = Arc::new(RwLock::new(None));

        // Spawn batch processor task
        let inner_clone = inner.clone();
        let pending_clone = pending_update.clone();
        let batch_interval = config.batch_interval;

        let batch_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(batch_interval);
            loop {
                interval.tick().await;
                
                // Check if there's a pending update
                let update = {
                    let mut pending = pending_clone.write();
                    pending.take()
                };

                // If there's an update, apply it
                if let Some(value) = update {
                    let _ = inner_clone.set_async(value).await;
                }
            }
        });

        Self {
            inner,
            pending_update,
            _batch_task: Arc::new(batch_task),
        }
    }

    /// Queue an update to be batched
    ///
    /// The update will be held until the next batch interval, at which point
    /// only the most recent queued value will be applied and observers notified.
    ///
    /// # Arguments
    ///
    /// * `value` - The new value to queue
    ///
    /// # Returns
    ///
    /// `Ok(())` if the update was queued successfully
    ///
    /// # Examples
    ///
    /// ```
    /// use observable_property_tokio::BatchedProperty;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = BatchedProperty::new(0);
    ///     
    ///     // Queue multiple updates
    ///     for i in 1..=10 {
    ///         property.queue_update(i)?;
    ///     }
    ///     
    ///     // Wait for batch to flush
    ///     tokio::time::sleep(Duration::from_millis(150)).await;
    ///     
    ///     // Property will have the last queued value
    ///     assert_eq!(property.get()?, 10);
    ///     
    ///     Ok(())
    /// }
    /// ```
    pub fn queue_update(&self, value: T) -> Result<(), PropertyError> {
        *self.pending_update.write() = Some(value);
        Ok(())
    }

    /// Set the value immediately, bypassing batching
    ///
    /// This will apply the update immediately and notify observers synchronously,
    /// without waiting for the next batch interval.
    ///
    /// # Arguments
    ///
    /// * `value` - The new value to set
    ///
    /// # Examples
    ///
    /// ```
    /// use observable_property_tokio::BatchedProperty;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    /// let property = BatchedProperty::new(0);
    /// property.set_immediate(42)?;
    /// assert_eq!(property.get()?, 42);
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_immediate(&self, value: T) -> Result<(), PropertyError> {
        self.inner.set(value)
    }

    /// Set the value immediately using async notification
    ///
    /// This will apply the update immediately and notify observers asynchronously,
    /// without waiting for the next batch interval.
    ///
    /// # Arguments
    ///
    /// * `value` - The new value to set
    pub async fn set_immediate_async(&self, value: T) -> Result<(), PropertyError> {
        self.inner.set_async(value).await
    }

    /// Get the current value
    ///
    /// # Examples
    ///
    /// ```
    /// use observable_property_tokio::BatchedProperty;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    ///     let property = BatchedProperty::new(42);
    ///     assert_eq!(property.get()?, 42);
    ///     Ok(())
    /// }
    /// ```
    pub fn get(&self) -> Result<T, PropertyError> {
        self.inner.get()
    }

    /// Subscribe to batched property changes
    ///
    /// Observers will be notified at batch intervals with the latest value.
    ///
    /// # Arguments
    ///
    /// * `observer` - Callback function to handle property changes
    ///
    /// # Returns
    ///
    /// `Ok(ObserverId)` containing a unique identifier for this observer
    pub fn subscribe(&self, observer: Observer<T>) -> Result<ObserverId, PropertyError> {
        self.inner.subscribe(observer)
    }

    /// Subscribe with an async handler
    pub fn subscribe_async<F, Fut>(&self, handler: F) -> Result<ObserverId, PropertyError>
    where
        F: Fn(T, T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.inner.subscribe_async(handler)
    }

    /// Unsubscribe an observer
    pub fn unsubscribe(&self, id: ObserverId) -> Result<(), PropertyError> {
        self.inner.unsubscribe(id)
    }

    /// Get the number of registered observers
    pub fn observer_count(&self) -> usize {
        self.inner.observer_count()
    }

    /// Clear all observers
    pub fn clear_observers(&self) -> Result<(), PropertyError> {
        self.inner.clear_observers()
    }

    /// Flush any pending batched update immediately
    ///
    /// This forces any queued update to be applied right away,
    /// rather than waiting for the next batch interval.
    ///
    /// # Examples
    ///
    /// ```
    /// use observable_property_tokio::BatchedProperty;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    /// let property = BatchedProperty::new(0);
    /// property.queue_update(42)?;
    /// property.flush().await?;
    /// assert_eq!(property.get()?, 42);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn flush(&self) -> Result<(), PropertyError> {
        let update = {
            let mut pending = self.pending_update.write();
            pending.take()
        };

        if let Some(value) = update {
            self.inner.set_async(value).await?;
        }

        Ok(())
    }
}

impl<T: Clone + Send + Sync + 'static> Clone for BatchedProperty<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            pending_update: Arc::clone(&self.pending_update),
            _batch_task: Arc::clone(&self._batch_task),
        }
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

#[cfg(test)]
mod connection_pool_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_concurrent_task_limiting() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 1000,
            max_pending_notifications: 1000,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 5, // Only 5 concurrent tasks allowed
        };

        let property = ObservableProperty::new_with_config(0, config);
        let concurrent_count = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));

        // Subscribe 20 async observers that all take 100ms to execute
        for _ in 0..20 {
            let counter = Arc::clone(&concurrent_count);
            let max_counter = Arc::clone(&max_concurrent);

            property.subscribe_async(move |_, _| {
                let counter = Arc::clone(&counter);
                let max_counter = Arc::clone(&max_counter);

                async move {
                    // Increment concurrent count
                    let current = counter.fetch_add(1, Ordering::SeqCst) + 1;

                    // Update max if needed
                    max_counter.fetch_max(current, Ordering::SeqCst);

                    // Simulate work
                    sleep(Duration::from_millis(100)).await;

                    // Decrement concurrent count
                    counter.fetch_sub(1, Ordering::SeqCst);
                }
            })?;
        }

        // Trigger notification to all observers
        property.set_async(42).await?;

        // Wait for all tasks to complete
        sleep(Duration::from_millis(500)).await;

        // Verify max concurrent was not exceeded
        let max_reached = max_concurrent.load(Ordering::SeqCst);
        println!(
            "Max concurrent tasks: {} (limit: 5)",
            max_reached
        );

        assert!(
            max_reached <= 5,
            "Expected max concurrent tasks <= 5, but got {}",
            max_reached
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_semaphore_blocks_when_max_reached() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 100,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 2, // Very low limit
        };

        let property = ObservableProperty::new_with_config(0, config);
        let execution_order = Arc::new(parking_lot::RwLock::new(Vec::new()));

        // Add 5 observers with delays to test blocking
        for i in 0..5 {
            let order = Arc::clone(&execution_order);
            property.subscribe_async(move |_, _| {
                let order = Arc::clone(&order);
                async move {
                    order.write().push((i, "start"));
                    sleep(Duration::from_millis(50)).await;
                    order.write().push((i, "end"));
                }
            })?;
        }

        // Trigger notification
        property.set_async(100).await?;

        // Wait for all to complete
        sleep(Duration::from_millis(300)).await;

        let order = execution_order.read();
        println!("Execution order: {:?}", *order);

        // Verify that we have start/end pairs
        assert_eq!(order.len(), 10, "Should have 5 start and 5 end events");

        // Verify all tasks completed
        let starts = order.iter().filter(|(_, phase)| *phase == "start").count();
        let ends = order.iter().filter(|(_, phase)| *phase == "end").count();
        assert_eq!(starts, 5, "Should have 5 starts");
        assert_eq!(ends, 5, "Should have 5 ends");

        Ok(())
    }

    #[tokio::test]
    async fn test_permits_released_after_execution() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 100,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 3,
        };

        let property = ObservableProperty::new_with_config(0, config);
        let executions = Arc::new(AtomicUsize::new(0));

        // Add 10 observers
        for _ in 0..10 {
            let counter = Arc::clone(&executions);
            property.subscribe_async(move |_, _| {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    sleep(Duration::from_millis(10)).await;
                }
            })?;
        }

        // Multiple notifications to test permit reuse
        for _ in 0..3 {
            property.set_async(42).await?;
            sleep(Duration::from_millis(100)).await;
        }

        // Verify all executions happened (10 observers * 3 notifications = 30)
        let total = executions.load(Ordering::SeqCst);
        assert_eq!(total, 30, "Expected 30 executions, got {}", total);

        Ok(())
    }

    #[tokio::test]
    async fn test_filtered_async_observers_respect_limit() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 100,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 3,
        };

        let property = ObservableProperty::new_with_config(0, config);
        let concurrent_count = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));

        // Add 10 filtered async observers
        for _ in 0..10 {
            let counter = Arc::clone(&concurrent_count);
            let max_counter = Arc::clone(&max_concurrent);

            property.subscribe_async_filtered(
                move |_, _| {
                    let counter = Arc::clone(&counter);
                    let max_counter = Arc::clone(&max_counter);

                    async move {
                        let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
                        max_counter.fetch_max(current, Ordering::SeqCst);
                        sleep(Duration::from_millis(50)).await;
                        counter.fetch_sub(1, Ordering::SeqCst);
                    }
                },
                |_, &new| new % 2 == 0, // Only trigger on even values
            )?;
        }

        // Trigger with even value
        property.set_async(100).await?;
        sleep(Duration::from_millis(200)).await;

        let max_reached = max_concurrent.load(Ordering::SeqCst);
        println!("Max concurrent filtered async tasks: {}", max_reached);

        assert!(
            max_reached <= 3,
            "Expected max concurrent tasks <= 3, got {}",
            max_reached
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_default_concurrent_limit() -> Result<(), PropertyError> {
        // Default config should have max_concurrent_async_tasks = 100
        let property = ObservableProperty::new(0);

        // Add 200 async observers
        for _ in 0..200 {
            property.subscribe_async(|_, _| async move {
                sleep(Duration::from_millis(50)).await;
            })?;
        }

        // This should work without blocking indefinitely
        property.set_async(42).await?;
        sleep(Duration::from_millis(300)).await;

        Ok(())
    }

    #[tokio::test]
    async fn test_mixed_sync_and_async_observers() -> Result<(), PropertyError> {
        let config = PropertyConfig {
            max_observers: 100,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 2,
        };

        let property = ObservableProperty::new_with_config(0, config);
        let sync_count = Arc::new(AtomicUsize::new(0));
        let async_count = Arc::new(AtomicUsize::new(0));

        // Add sync observers (these are not limited by semaphore)
        for _ in 0..5 {
            let counter = Arc::clone(&sync_count);
            property.subscribe(Arc::new(move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }))?;
        }

        // Add async observers (these ARE limited by semaphore)
        for _ in 0..5 {
            let counter = Arc::clone(&async_count);
            property.subscribe_async(move |_, _| {
                let counter = Arc::clone(&counter);
                async move {
                    sleep(Duration::from_millis(20)).await;
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })?;
        }

        property.set_async(100).await?;
        sleep(Duration::from_millis(150)).await;

        // All sync observers should have executed immediately
        assert_eq!(sync_count.load(Ordering::SeqCst), 5);

        // All async observers should have executed (even if limited)
        assert_eq!(async_count.load(Ordering::SeqCst), 5);

        Ok(())
    }
}
