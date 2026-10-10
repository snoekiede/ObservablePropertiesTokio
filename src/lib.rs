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

use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::panic;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::task::{JoinError, JoinSet};

const MAX_IN_FLIGHT_NOTIFICATIONS: usize = 256;

/// Errors that can occur when working with ObservableProperty
#[derive(Error, Debug, Clone)]
pub enum PropertyError {
    /// Failed to acquire a read lock on the property
    #[error("Failed to acquire read lock: {context}")]
    ReadLockError {
        /// Context describing what operation was being attempted
        context: String,
    },

    /// Failed to acquire a write lock on the property
    #[error("Failed to acquire write lock: {context}")]
    WriteLockError {
        /// Context describing what operation was being attempted
        context: String,
    },

    /// Attempted to unsubscribe an observer that doesn't exist
    #[error("Observer with ID {id} not found")]
    ObserverNotFound {
        /// The ID of the observer that wasn't found
        id: ObserverId,
    },

    /// The property's lock has been poisoned due to a panic in another thread
    #[error("Property is in a poisoned state due to a panic in another thread")]
    PoisonedLock,

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

    /// Notification capacity is full; retry after outstanding notifications complete
    #[error("Notification capacity is full ({capacity} in-flight notifications)")]
    Backpressure { capacity: usize },
}

/// Function type for observers that get called when property values change
pub type Observer<T> = Arc<dyn Fn(&T, &T) + Send + Sync>;
type AsyncObserver<T> = Arc<dyn Fn(T, T) -> Option<AsyncFuture> + Send + Sync>;
type AsyncFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

struct AsyncNotification {
    futures: Vec<AsyncFuture>,
    completion: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    error: Option<String>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

#[derive(Clone)]
enum ObserverCallback<T> {
    Sync(Observer<T>),
    Async(AsyncObserver<T>),
}

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
    keepalive: Arc<Mutex<Vec<Box<dyn Send + Sync>>>>,
}

struct InnerProperty<T> {
    value: T,
    observers: HashMap<ObserverId, ObserverCallback<T>>,
    next_id: usize,
    pending_notifications: VecDeque<Notification<T>>,
    notifying: bool,
    async_sender: Option<tokio::sync::mpsc::Sender<AsyncNotification>>,
    notification_slots: Arc<tokio::sync::Semaphore>,
}

struct Notification<T> {
    old_value: T,
    new_value: T,
    observers: Vec<ObserverCallback<T>>,
    completion: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    runtime: Option<tokio::runtime::Handle>,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
}

fn try_notification_permit<T>(
    inner: &Arc<RwLock<InnerProperty<T>>>,
) -> Result<tokio::sync::OwnedSemaphorePermit, PropertyError> {
    inner
        .read()
        .notification_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| PropertyError::Backpressure {
            capacity: MAX_IN_FLIGHT_NOTIFICATIONS,
        })
}

fn replace_value<T, F>(
    inner: &Arc<RwLock<InnerProperty<T>>>,
    update_fn: F,
    completion: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    runtime: Option<tokio::runtime::Handle>,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
) -> bool
where
    T: Clone,
    F: FnOnce(T) -> T,
{
    let mut inner = inner.write();
    let current_value = inner.value.clone();
    let new_value = update_fn(current_value);
    let old_value = std::mem::replace(&mut inner.value, new_value.clone());
    let observers = inner.observers.values().cloned().collect();
    inner.pending_notifications.push_back(Notification {
        old_value,
        new_value,
        observers,
        completion,
        runtime,
        permit,
    });

    if inner.notifying {
        false
    } else {
        inner.notifying = true;
        true
    }
}

fn dispatch_pending<T: Clone>(inner: &Arc<RwLock<InnerProperty<T>>>) {
    loop {
        let notification = {
            let mut inner = inner.write();
            match inner.pending_notifications.pop_front() {
                Some(notification) => notification,
                None => {
                    inner.notifying = false;
                    return;
                }
            }
        };

        dispatch_notification(notification, inner);
    }
}

fn dispatch_notification<T: Clone>(
    notification: Notification<T>,
    inner: &Arc<RwLock<InnerProperty<T>>>,
) {
    let Notification {
        old_value,
        new_value,
        observers,
        completion,
        runtime,
        permit,
    } = notification;
    let mut async_futures = Vec::new();
    let mut async_error = None;

    for observer in observers {
        match observer {
            ObserverCallback::Sync(observer) => {
                if let Err(error) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                    observer(&old_value, &new_value);
                })) {
                    eprintln!("Observer panic: {error:?}");
                }
            }
            ObserverCallback::Async(observer) => {
                match panic::catch_unwind(panic::AssertUnwindSafe(|| {
                    observer(old_value.clone(), new_value.clone())
                })) {
                    Ok(Some(future)) => async_futures.push(future),
                    Ok(None) => {}
                    Err(_) => {
                        async_error.get_or_insert_with(|| {
                            "Async observer panicked before returning its future".to_string()
                        });
                    }
                }
            }
        }
    }

    if async_futures.is_empty() && async_error.is_none() {
        if let Some(completion) = completion {
            let _ = completion.send(Ok(()));
        }
        return;
    }

    let permit = match permit {
        Some(permit) => Some(permit),
        None => match inner.read().notification_slots.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                let error = format!(
                    "Notification capacity is full ({MAX_IN_FLIGHT_NOTIFICATIONS} in-flight notifications)"
                );
                if let Some(completion) = completion {
                    let _ = completion.send(Err(error));
                } else {
                    eprintln!("Async observer notification rejected: {error}");
                }
                return;
            }
        },
    };

    let Some(runtime) = runtime else {
        if let Some(completion) = completion {
            let _ = completion.send(Err(
                "No Tokio runtime was available to execute async observers".to_string(),
            ));
        } else {
            eprintln!("Cannot run async observer without an active Tokio runtime");
        }
        return;
    };

    let (sender, receiver) = {
        let mut inner = inner.write();
        match inner.async_sender.as_ref() {
            Some(sender) if !sender.is_closed() => (sender.clone(), None),
            _ => {
                let (sender, receiver) = tokio::sync::mpsc::channel(MAX_IN_FLIGHT_NOTIFICATIONS);
                inner.async_sender = Some(sender.clone());
                (sender, Some(receiver))
            }
        }
    };

    if let Some(receiver) = receiver {
        runtime.spawn(run_async_notifications(receiver));
    }
    let notification = AsyncNotification {
        futures: async_futures,
        completion,
        error: async_error,
        _permit: match permit {
            Some(permit) => permit,
            None => return,
        },
    };
    if let Err(error) = sender.try_send(notification) {
        let notification = error.into_inner();
        if let Some(completion) = notification.completion {
            let _ = completion.send(Err(
                "Async observer worker stopped before accepting a notification".to_string(),
            ));
        } else {
            eprintln!("Async observer worker stopped before accepting a notification");
        }
    }
}

async fn run_async_notifications(mut receiver: tokio::sync::mpsc::Receiver<AsyncNotification>) {
    while let Some(notification) = receiver.recv().await {
        tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            for future in notification.futures {
                tasks.spawn(future);
            }

            let mut first_error = notification.error;
            while let Some(result) = tasks.join_next().await {
                if let Err(error) = result {
                    first_error.get_or_insert_with(|| error.to_string());
                }
            }

            let result = first_error.map_or(Ok(()), Err);
            drop(notification._permit);
            if let Some(completion) = notification.completion {
                let _ = completion.send(result);
            } else if let Err(error) = result {
                eprintln!("Detached async observer failed: {error}");
            }
        });
    }
}

async fn await_async_observers(
    receiver: tokio::sync::oneshot::Receiver<Result<(), String>>,
) -> Result<(), PropertyError> {
    receiver
        .await
        .map_err(|error| PropertyError::JoinError(error.to_string()))?
        .map_err(PropertyError::JoinError)
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
                pending_notifications: VecDeque::new(),
                notifying: false,
                async_sender: None,
                notification_slots: Arc::new(tokio::sync::Semaphore::new(
                    MAX_IN_FLIGHT_NOTIFICATIONS,
                )),
            })),
            keepalive: Arc::new(Mutex::new(Vec::new())),
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
    /// 2. Update the value and enqueue a snapshot of observers in commit order
    /// 3. Release the lock
    /// 4. Dispatch notifications sequentially in FIFO order
    ///
    /// Notifications are wrapped in panic recovery to prevent one misbehaving
    /// synchronous observer from affecting others. If another caller is already
    /// dispatching notifications, this call enqueues its notification and may
    /// return before that notification is delivered.
    /// At most 256 notifications may be queued or running; when capacity is exhausted,
    /// this method returns [`PropertyError::Backpressure`] without changing the value.
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
        let permit = try_notification_permit(&self.inner)?;
        let runtime = tokio::runtime::Handle::try_current().ok();
        if replace_value(&self.inner, |_| new_value, None, runtime, Some(permit)) {
            dispatch_pending(&self.inner);
        }
        Ok(())
    }

    /// Sets the property and awaits all registered async observers.
    ///
    /// Notifications are dispatched in FIFO commit order. Synchronous observers
    /// run on the current dispatcher. Async observers for one update run concurrently;
    /// separate batches start in FIFO order but may complete out of order. This method
    /// waits for its update's async observers. If notification capacity is exhausted,
    /// returns [`PropertyError::Backpressure`] without changing the value.
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
    ///     let property = ObservableProperty::new(0);
    ///
    ///     property.subscribe_async(|old, new| async move {
    ///         // This observer can perform async work
    ///         println!("Slow observer: {} -> {}", old, new);
    ///     })?;
    ///
    ///     // Waits for the async observer to finish
    ///     property.set_async(42).await?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn set_async(&self, new_value: T) -> Result<(), PropertyError> {
        let permit = try_notification_permit(&self.inner)?;
        let runtime =
            tokio::runtime::Handle::try_current().map_err(|error| PropertyError::TokioError {
                reason: error.to_string(),
            })?;
        let (completion, receiver) = tokio::sync::oneshot::channel();
        if replace_value(
            &self.inner,
            |_| new_value,
            Some(completion),
            Some(runtime),
            Some(permit),
        ) {
            dispatch_pending(&self.inner);
        }
        await_async_observers(receiver).await
    }

    /// Update the property value using a closure that has access to the current value
    ///
    /// This is a more ergonomic way to update a property based on its current value,
    /// without having to call `get()` and `set()` separately. The closure runs while
    /// the property is write-locked to make the update atomic, so it must not re-enter
    /// this property.
    /// Returns [`PropertyError::Backpressure`] without changing the value if all
    /// 256 notification slots are occupied.
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
        // The update closure runs under the write lock so concurrent updates cannot
        // overwrite one another. It must not re-enter this property.
        let permit = try_notification_permit(&self.inner)?;
        let runtime = tokio::runtime::Handle::try_current().ok();
        if replace_value(&self.inner, update_fn, None, runtime, Some(permit)) {
            dispatch_pending(&self.inner);
        }
        Ok(())
    }

    /// Update the property value asynchronously using a closure
    ///
    /// Like `update()` but awaits async observers after atomically updating the value.
    /// The update closure runs under the write lock and must not re-enter this property.
    /// Returns [`PropertyError::Backpressure`] without changing the value if all
    /// 256 notification slots are occupied.
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
        let permit = try_notification_permit(&self.inner)?;
        let runtime =
            tokio::runtime::Handle::try_current().map_err(|error| PropertyError::TokioError {
                reason: error.to_string(),
            })?;
        // Compute and publish atomically before awaiting observer work.
        let (completion, receiver) = tokio::sync::oneshot::channel();
        if replace_value(
            &self.inner,
            update_fn,
            Some(completion),
            Some(runtime),
            Some(permit),
        ) {
            dispatch_pending(&self.inner);
        }
        await_async_observers(receiver).await
    }

    /// Subscribes an observer function to be called when the property changes
    ///
    /// The observer function will be called with the old and new values whenever
    /// the property is modified. Async observer work is detached when a property
    /// is changed with `set()`; use `set_async()` to await completion and receive
    /// task join errors.
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
        inner.next_id =
            inner
                .next_id
                .checked_add(1)
                .ok_or_else(|| PropertyError::ObserverError {
                    reason: "Observer ID space exhausted".to_string(),
                })?;
        inner.observers.insert(id, ObserverCallback::Sync(observer));
        Ok(id)
    }

    fn subscribe_async_callback(
        &self,
        observer: AsyncObserver<T>,
    ) -> Result<ObserverId, PropertyError> {
        let mut inner = self.inner.write();

        let id = ObserverId(inner.next_id);
        inner.next_id =
            inner
                .next_id
                .checked_add(1)
                .ok_or_else(|| PropertyError::ObserverError {
                    reason: "Observer ID space exhausted".to_string(),
                })?;
        inner
            .observers
            .insert(id, ObserverCallback::Async(observer));
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
    ///     Ok(())
    /// }
    /// ```
    pub fn subscribe_async<F, Fut>(&self, handler: F) -> Result<ObserverId, PropertyError>
    where
        F: Fn(T, T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let handler = Arc::new(handler);
        let observer = Arc::new(move |old: T, new: T| {
            let handler_clone = Arc::clone(&handler);
            Some(Box::pin(async move {
                handler_clone(old, new).await;
            }) as Pin<Box<dyn Future<Output = ()> + Send>>)
        });

        self.subscribe_async_callback(observer)
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

        let observer = Arc::new(move |old: T, new: T| {
            if filter(&old, &new) {
                let handler_clone = Arc::clone(&handler);
                Some(Box::pin(async move { handler_clone(old, new).await })
                    as Pin<Box<dyn Future<Output = ()> + Send>>)
            } else {
                None
            }
        });

        self.subscribe_async_callback(observer)
    }

    /// Create a new ObservableProperty with transformation applied to the value
    ///
    /// This creates a derived property that tracks changes to the original property,
    /// with a transformation applied. The result is an ordinary property and can also
    /// be changed directly. The source observer is removed when the final clone of the
    /// derived property is dropped.
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

        let derived_inner = Arc::downgrade(&derived.inner);
        let id = self.subscribe(Arc::new(move |_, new_value| {
            if let Some(inner) = derived_inner.upgrade() {
                let transformed = transform(new_value);
                let runtime = tokio::runtime::Handle::try_current().ok();
                match try_notification_permit(&inner) {
                    Ok(permit) => {
                        if replace_value(&inner, |_| transformed, None, runtime, Some(permit)) {
                            dispatch_pending(&inner);
                        }
                    }
                    Err(error) => {
                        eprintln!(
                            "Mapped-property async notifications are under backpressure: {error}"
                        );
                        let (old_value, new_value, observers) = {
                            let mut inner = inner.write();
                            let old_value =
                                std::mem::replace(&mut inner.value, transformed.clone());
                            (
                                old_value,
                                transformed,
                                inner.observers.values().cloned().collect(),
                            )
                        };
                        dispatch_notification(
                            Notification {
                                old_value,
                                new_value,
                                observers,
                                completion: None,
                                runtime,
                                permit: None,
                            },
                            &inner,
                        );
                    }
                }
            }
        }))?;
        derived.keepalive.lock().push(Box::new(Subscription {
            inner: Arc::clone(&self.inner),
            id,
        }));

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
    /// This performs the same operation as `clear_observers()`. Async observer work
    /// that has already been queued or started is not cancelled.
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
        self.clear_observers()
    }

    pub fn subscribe_with_token(
        &self,
        observer: Observer<T>,
    ) -> Result<Subscription<T>, PropertyError> {
        let id = self.subscribe(observer)?;
        Ok(Subscription {
            inner: Arc::clone(&self.inner),
            id,
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
            id,
        })
    }

    pub fn subscribe_async_with_token<F, Fut>(
        &self,
        handler: F,
    ) -> Result<Subscription<T>, PropertyError>
    where
        F: Fn(T, T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let id = self.subscribe_async(handler)?;
        Ok(Subscription {
            inner: Arc::clone(&self.inner),
            id,
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
            id,
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
            keepalive: Arc::clone(&self.keepalive),
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

impl From<JoinError> for PropertyError {
    fn from(err: JoinError) -> Self {
        PropertyError::JoinError(err.to_string())
    }
}

impl<T: Clone + std::fmt::Debug + Send + Sync + 'static> std::fmt::Debug for ObservableProperty<T> {
    /// Debug implementation that shows the current value if accessible
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.get() {
            Ok(value) => f
                .debug_struct("ObservableProperty")
                .field("value", &value)
                .field("observers_count", &self.observer_count())
                .finish(),
            Err(_) => f
                .debug_struct("ObservableProperty")
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_updates_are_atomic() -> Result<(), PropertyError> {
        let property = Arc::new(ObservableProperty::new(0));
        let mut tasks = Vec::new();

        for _ in 0..8 {
            let property = Arc::clone(&property);
            tasks.push(tokio::task::spawn_blocking(move || {
                for _ in 0..250 {
                    loop {
                        match property.update(|value| value + 1) {
                            Ok(()) => break,
                            Err(PropertyError::Backpressure { .. }) => {
                                std::thread::yield_now();
                            }
                            Err(error) => panic!("unexpected update error: {error}"),
                        }
                    }
                }
            }));
        }

        for task in tasks {
            task.await.map_err(PropertyError::from)?;
        }

        assert_eq!(property.get()?, 2_000);
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
    async fn test_map_unsubscribes_when_last_derived_clone_drops() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(10);
        let derived = property.map(|value| value * 2)?;
        let derived_clone = derived.clone();
        assert_eq!(property.observer_count(), 1);

        drop(derived);
        assert_eq!(property.observer_count(), 1);
        drop(derived_clone);
        assert_eq!(property.observer_count(), 0);
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
    async fn test_async_observer_can_await_nested_update() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let observed_property = property.clone();
        property.subscribe_async(move |_, new_value| {
            let property = observed_property.clone();
            async move {
                if new_value == 1 {
                    property.set_async(2).await.unwrap();
                }
            }
        })?;

        property.set_async(1).await?;
        assert_eq!(property.get()?, 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_notification_capacity_returns_backpressure() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        property.subscribe_async(|_, _| async {
            std::future::pending::<()>().await;
        })?;

        for value in 1..=MAX_IN_FLIGHT_NOTIFICATIONS {
            property.set(value)?;
        }

        assert!(matches!(
            property.set(MAX_IN_FLIGHT_NOTIFICATIONS + 1),
            Err(PropertyError::Backpressure { capacity }) if capacity == MAX_IN_FLIGHT_NOTIFICATIONS
        ));
        assert_eq!(property.get()?, MAX_IN_FLIGHT_NOTIFICATIONS);
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
    async fn test_set_async_waits_for_async_observers() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));
        let observer_counter = Arc::clone(&counter);

        property.subscribe_async(move |_, _| {
            let counter = Arc::clone(&observer_counter);
            async move {
                sleep(Duration::from_millis(10)).await;
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })?;

        property.set_async(1).await?;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_async_observer_batches_start_in_update_order() -> Result<(), PropertyError> {
        let property = Arc::new(ObservableProperty::new(0));
        let first_started = Arc::new(tokio::sync::Notify::new());
        let release_first = Arc::new(tokio::sync::Notify::new());
        let observed = Arc::new(Mutex::new(Vec::new()));

        let started = Arc::clone(&first_started);
        let release = Arc::clone(&release_first);
        let observed_clone = Arc::clone(&observed);
        property.subscribe_async(move |_, new_value| {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            let observed = Arc::clone(&observed_clone);
            async move {
                if new_value == 1 {
                    started.notify_one();
                    release.notified().await;
                }
                observed.lock().push(new_value);
            }
        })?;

        let first_property = Arc::clone(&property);
        let first = tokio::spawn(async move { first_property.set_async(1).await });
        first_started.notified().await;

        let second_property = Arc::clone(&property);
        let second = tokio::spawn(async move { second_property.set_async(2).await });
        while property.get()? != 2 {
            tokio::task::yield_now().await;
        }

        second.await.map_err(PropertyError::from)??;
        assert_eq!(*observed.lock(), vec![2]);
        release_first.notify_one();
        first.await.map_err(PropertyError::from)??;

        assert_eq!(*observed.lock(), vec![2, 1]);
        Ok(())
    }

    #[test]
    fn test_concurrent_notifications_are_dispatched_in_commit_order() -> Result<(), PropertyError> {
        let property = Arc::new(ObservableProperty::new(0));
        let callback_entered = Arc::new(std::sync::Barrier::new(2));
        let release_callback = Arc::new(std::sync::Barrier::new(2));
        let observed = Arc::new(Mutex::new(Vec::new()));

        let callback_property = Arc::clone(&property);
        let callback_entered_clone = Arc::clone(&callback_entered);
        let release_callback_clone = Arc::clone(&release_callback);
        let observed_clone = Arc::clone(&observed);
        property.subscribe(Arc::new(move |_, new_value| {
            if *new_value == 1 {
                callback_entered_clone.wait();
                release_callback_clone.wait();
            }
            observed_clone.lock().push(*new_value);
        }))?;

        let first_update = std::thread::spawn(move || callback_property.set(1));
        callback_entered.wait();
        property.set(2)?;
        release_callback.wait();
        first_update.join().expect("first update thread panicked")?;

        assert_eq!(*observed.lock(), vec![1, 2]);
        Ok(())
    }

    #[test]
    fn test_mapped_property_stays_in_sync_with_concurrent_updates() -> Result<(), PropertyError> {
        let property = Arc::new(ObservableProperty::new(0));
        let transform_entered = Arc::new(std::sync::Barrier::new(2));
        let release_transform = Arc::new(std::sync::Barrier::new(2));

        let entered_clone = Arc::clone(&transform_entered);
        let release_clone = Arc::clone(&release_transform);
        let mapped = property.map(move |value| {
            if *value == 1 {
                entered_clone.wait();
                release_clone.wait();
            }
            *value
        })?;

        let update_property = Arc::clone(&property);
        let first_update = std::thread::spawn(move || update_property.set(1));
        transform_entered.wait();
        property.set(2)?;
        release_transform.wait();
        first_update.join().expect("first update thread panicked")?;

        assert_eq!(property.get()?, 2);
        assert_eq!(mapped.get()?, 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_set_async_reports_async_observer_panics() -> Result<(), PropertyError> {
        let property = ObservableProperty::new(0);
        property.subscribe_async(|_, _| async {
            panic!("async observer panic");
        })?;

        assert!(matches!(
            property.set_async(1).await,
            Err(PropertyError::JoinError(_))
        ));
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
            |old, new| new > old,
        )?;

        property.set_async(10).await?; // Should trigger (0 -> 10)
        property.set_async(5).await?; // Should NOT trigger (10 -> 5)
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
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }))?;

        property.subscribe(Arc::new({
            let counter = counter2.clone();
            move |_, _| {
                counter.fetch_add(2, Ordering::SeqCst);
            }
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
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
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
            Err(PropertyError::ObserverNotFound { .. }) => {}
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
                move |_, _| {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }),
            |old, new| new > old, // Only trigger when value increases
        )?;

        property.set(10)?; // Should trigger (0 -> 10)
        property.set(5)?; // Should NOT trigger (10 -> 5)
        property.set(15)?; // Should trigger (5 -> 15)

        assert_eq!(counter.load(Ordering::SeqCst), 2);
        Ok(())
    }

    // Concurrent access tests
    #[tokio::test]
    async fn test_concurrent_modifications() -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    {
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
                prop.set(i)
                    .map_err(|e| format!("Failed to set property: {}", e))
            });
            tasks.push(task);
        }

        // Wait for all tasks to complete
        for task in tasks {
            task.await??;
        }

        // Final value should be one of the set values (1-5)
        let final_value = final_counter.load(Ordering::SeqCst);
        assert!((1..=5).contains(&final_value));
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
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
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
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
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
                move |_, _| {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }),
            |old, new| old.name != new.name, // Only notify on name changes
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
                move |_, _| {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
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
                    move |_, _| {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                }),
                filter,
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
                |old, new| new > old, // Only trigger when value increases
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
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }))?;

        // Second subscription
        let subscription2 = property.subscribe_with_token(Arc::new({
            let counter = counter2.clone();
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
            }
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
                move |_, _| {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
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
