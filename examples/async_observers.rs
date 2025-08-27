use observable_property_tokio::ObservableProperty;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    println!("=== Async Observers Example ===");

    let property = ObservableProperty::new(0);
    let counter = Arc::new(AtomicUsize::new(0));

    // Regular synchronous observer
    property.subscribe(Arc::new(|old, new| {
        println!("🔄 Sync observer: {} -> {}", old, new);
    }))?;

    // Async observer that simulates some async work
    let counter_clone = counter.clone();
    property.subscribe_async(move |old, new| {
        let counter = counter_clone.clone();
        async move {
            println!("⏳ Async observer starting: {} -> {}", old, new);

            // Simulate some async work
            sleep(Duration::from_millis(100)).await;

            counter.fetch_add(1, Ordering::SeqCst);
            println!("✅ Async observer completed: {} -> {}", old, new);
        }
    })?;

    // Another async observer with different timing
    property.subscribe_async(move |old, new| async move {
        println!("💤 Slow async observer starting: {} -> {}", old, new);

        // Simulate slower async work
        sleep(Duration::from_millis(200)).await;

        println!("🐌 Slow async observer completed: {} -> {}", old, new);
    })?;

    println!("\n--- Setting values with async observers ---");

    println!("\nSetting to 1:");
    property.set_async(1).await?;

    println!("\nSetting to 2:");
    property.set_async(2).await?;

    println!("\nSetting to 3:");
    property.set_async(3).await?;

    // Wait for all async observers to complete
    println!("\nWaiting for async observers to complete...");
    sleep(Duration::from_millis(300)).await;

    let final_count = counter.load(Ordering::SeqCst);
    println!("\nAsync observer executed {} times", final_count);

    println!("Final property value: {}", property.get()?);

    Ok(())
}
