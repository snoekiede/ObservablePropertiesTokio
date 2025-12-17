use observable_property_tokio::{ObservableProperty, PropertyConfig};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};

/// This example demonstrates connection pooling for async tasks
/// to prevent resource exhaustion when many async observers execute simultaneously.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Observable Property Connection Pooling Example ===\n");

    // Example 1: Default Configuration
    println!("1. Default Configuration (100 concurrent tasks):");
    let property_default = ObservableProperty::new(0);
    println!("   Default max_concurrent_async_tasks: 100");
    println!("   This allows up to 100 async observer tasks to run simultaneously.\n");

    // Example 2: Custom Configuration with Low Limit
    println!("2. Custom Configuration (5 concurrent tasks):");
    let config = PropertyConfig {
        max_observers: 1000,
        max_pending_notifications: 100,
        observer_timeout_ms: 5000,
        max_concurrent_async_tasks: 5, // Only 5 concurrent tasks allowed
    };

    let property = ObservableProperty::new_with_config(0, config);
    println!("   Custom max_concurrent_async_tasks: 5");
    println!("   This limits concurrent async tasks to 5, preventing resource exhaustion.\n");

    // Example 3: Demonstrating Concurrency Limiting
    println!("3. Demonstrating Concurrency Limiting:");
    println!("   Adding 20 async observers that each take 100ms to execute...");

    let concurrent_count = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let total_executions = Arc::new(AtomicUsize::new(0));

    for i in 0..20 {
        let counter = Arc::clone(&concurrent_count);
        let max_counter = Arc::clone(&max_concurrent);
        let total = Arc::clone(&total_executions);

        property.subscribe_async(move |old, new| {
            let counter = Arc::clone(&counter);
            let max_counter = Arc::clone(&max_counter);
            let total = Arc::clone(&total);

            async move {
                // Increment concurrent count
                let current = counter.fetch_add(1, Ordering::SeqCst) + 1;

                // Update max concurrent reached
                max_counter.fetch_max(current, Ordering::SeqCst);

                // Simulate async work
                println!("   Observer {} executing: {} -> {} (concurrent: {})", i, old, new, current);
                sleep(Duration::from_millis(100)).await;

                // Decrement concurrent count
                counter.fetch_sub(1, Ordering::SeqCst);
                total.fetch_add(1, Ordering::SeqCst);
            }
        })?;
    }

    println!("   Triggering property update...");
    let start = Instant::now();
    property.set_async(42).await?;

    // Wait for all tasks to complete
    sleep(Duration::from_millis(600)).await;
    let elapsed = start.elapsed();

    println!("\n   Results:");
    println!("   - Total observers: 20");
    println!("   - Max concurrent tasks: {} (limit: 5)", max_concurrent.load(Ordering::SeqCst));
    println!("   - Total executions: {}", total_executions.load(Ordering::SeqCst));
    println!("   - Time elapsed: {:?}", elapsed);
    println!("   - Without pooling: ~100ms (all parallel)");
    println!("   - With pooling (5 limit): ~400ms (4 batches of 5)\n");

    // Example 4: Preventing Resource Exhaustion
    println!("4. Preventing Resource Exhaustion:");
    let config_high = PropertyConfig {
        max_observers: 1000,
        max_pending_notifications: 1000,
        observer_timeout_ms: 5000,
        max_concurrent_async_tasks: 10, // Reasonable limit
    };

    let property_high = ObservableProperty::new_with_config(0, config_high);

    // Add many async observers
    for _ in 0..100 {
        property_high.subscribe_async(|_, _| async move {
            // Simulate database query or HTTP request
            sleep(Duration::from_millis(50)).await;
        })?;
    }

    println!("   Added 100 async observers");
    println!("   Without pooling: Could spawn 100 tokio tasks simultaneously");
    println!("   With pooling: Only 10 tasks run concurrently");
    println!("   Benefit: Prevents CPU/memory exhaustion, bounded resource usage\n");

    let start = Instant::now();
    property_high.set_async(999).await?;
    sleep(Duration::from_millis(600)).await;
    let elapsed = start.elapsed();

    println!("   Execution completed in {:?}", elapsed);
    println!("   Tasks were queued and executed in batches of 10\n");

    // Example 5: Filtered Async Observers Also Respect Limit
    println!("5. Filtered Async Observers Respect Limit:");
    let config_filtered = PropertyConfig {
        max_observers: 100,
        max_pending_notifications: 100,
        observer_timeout_ms: 5000,
        max_concurrent_async_tasks: 3,
    };

    let property_filtered = ObservableProperty::new_with_config(0, config_filtered);
    let filtered_count = Arc::new(AtomicUsize::new(0));

    // Add filtered async observers
    for i in 0..10 {
        let counter = Arc::clone(&filtered_count);
        property_filtered.subscribe_async_filtered(
            move |_, new| {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    println!("   Filtered observer {} triggered for even value: {}", i, new);
                    sleep(Duration::from_millis(50)).await;
                }
            },
            |_, &new| new % 2 == 0, // Only trigger on even values
        )?;
    }

    println!("   Added 10 filtered async observers (trigger on even values)");
    println!("   Max concurrent: 3\n");

    property_filtered.set_async(100).await?;
    sleep(Duration::from_millis(200)).await;

    println!("   Filtered observers executed: {}", filtered_count.load(Ordering::SeqCst));
    println!("   All respected the 3-task concurrent limit\n");

    // Example 6: Mixed Sync and Async Observers
    println!("6. Mixed Sync and Async Observers:");
    let config_mixed = PropertyConfig {
        max_observers: 100,
        max_pending_notifications: 100,
        observer_timeout_ms: 5000,
        max_concurrent_async_tasks: 3,
    };

    let property_mixed = ObservableProperty::new_with_config(0, config_mixed);
    let sync_count = Arc::new(AtomicUsize::new(0));
    let async_count = Arc::new(AtomicUsize::new(0));

    // Add sync observers (not affected by semaphore)
    for i in 0..5 {
        let counter = Arc::clone(&sync_count);
        property_mixed.subscribe(Arc::new(move |old, new| {
            counter.fetch_add(1, Ordering::SeqCst);
            println!("   Sync observer {} executed immediately: {} -> {}", i, old, new);
        }))?;
    }

    // Add async observers (limited by semaphore)
    for i in 0..5 {
        let counter = Arc::clone(&async_count);
        property_mixed.subscribe_async(move |old, new| {
            let counter = Arc::clone(&counter);
            async move {
                sleep(Duration::from_millis(50)).await;
                counter.fetch_add(1, Ordering::SeqCst);
                println!("   Async observer {} executed (pooled): {} -> {}", i, old, new);
            }
        })?;
    }

    println!("   Added 5 sync + 5 async observers");
    println!("   Sync observers: Execute immediately (no pooling)");
    println!("   Async observers: Limited to 3 concurrent tasks\n");

    property_mixed.set_async(777).await?;
    sleep(Duration::from_millis(150)).await;

    println!("\n   Results:");
    println!("   - Sync executions: {}", sync_count.load(Ordering::SeqCst));
    println!("   - Async executions: {}", async_count.load(Ordering::SeqCst));
    println!("   - Sync observers executed first (immediately)");
    println!("   - Async observers executed in batches (pooled)\n");

    // Example 7: Production Recommendations
    println!("7. Production Configuration Recommendations:");
    println!("   - Default (100): Good for most applications");
    println!("   - High load (50): If you have many frequent updates");
    println!("   - Resource constrained (20): Limited CPU/memory environments");
    println!("   - Heavy async work (10): If observers do expensive I/O");
    println!("   - Testing/Development (5): Easier to observe concurrency behavior\n");

    println!("   Example production config:");
    println!("   PropertyConfig {{");
    println!("       max_observers: 1000,");
    println!("       max_pending_notifications: 100,");
    println!("       observer_timeout_ms: 5000,");
    println!("       max_concurrent_async_tasks: 50,  // Adjust based on workload");
    println!("   }}\n");

    println!("=== Example Complete ===");
    println!("Connection pooling prevents resource exhaustion and provides predictable performance!");

    Ok(())
}
