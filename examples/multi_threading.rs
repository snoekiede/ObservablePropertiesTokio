use observable_property_tokio::ObservableProperty;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::task;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    println!("=== Multi-threading Example ===");

    let property = Arc::new(ObservableProperty::new(0));
    let update_counter = Arc::new(AtomicUsize::new(0));

    // Subscribe to track all changes
    let counter_clone = update_counter.clone();
    property.subscribe(Arc::new(move |old, new| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        println!("🔄 Observer: {} -> {} (thread: {:?})", old, new, std::thread::current().id());
    }))?;

    println!("\n--- Spawning multiple tasks to update the property ---");

    // Create multiple tasks that will update the property concurrently
    let mut handles = vec![];

    for i in 1..=5 {
        let prop = property.clone();
        let handle = task::spawn(async move {
            println!("📤 Task {} setting value to {}", i, i * 10);

            if let Err(e) = prop.set(i * 10) {
                eprintln!("❌ Task {} failed to set value: {}", i, e);
            } else {
                println!("✅ Task {} completed", i);
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for (i, handle) in handles.into_iter().enumerate() {
        handle.await.expect(&format!("Task {} panicked", i + 1));
    }

    // Create more tasks using async setting
    println!("\n--- Testing async updates from multiple tasks ---");
    let mut async_handles = vec![];

    for i in 6..=10 {
        let prop = property.clone();
        let handle = task::spawn(async move {
            println!("📤 Async task {} setting value to {}", i, i * 10);

            if let Err(e) = prop.set_async(i * 10).await {
                eprintln!("❌ Async task {} failed to set value: {}", i, e);
            } else {
                println!("✅ Async task {} completed", i);
            }
        });
        async_handles.push(handle);
    }

    // Wait for all async tasks to complete
    for (i, handle) in async_handles.into_iter().enumerate() {
        handle.await.expect(&format!("Async task {} panicked", i + 6));
    }

    // Give a moment for any remaining async observers to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let final_value = property.get()?;
    let total_updates = update_counter.load(Ordering::SeqCst);

    println!("\n--- Results ---");
    println!("Final property value: {}", final_value);
    println!("Total observer notifications: {}", total_updates);
    println!("Property is thread-safe and handled {} concurrent updates!", total_updates);

    Ok(())
}
