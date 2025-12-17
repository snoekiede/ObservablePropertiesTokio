use observable_property_tokio::ObservableProperty;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::sleep;

/// This example demonstrates graceful shutdown with timeout,
/// showing how to properly clean up resources and wait for
/// pending async operations to complete.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Graceful Shutdown with Timeout Example ===\n");

    // Example 1: Basic shutdown with timeout
    println!("1. Basic Shutdown with Timeout:");
    {
        let property = ObservableProperty::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        // Add some observers
        let counter_clone = counter.clone();
        property.subscribe(Arc::new(move |old, new| {
            println!("   Observer 1: {} -> {}", old, new);
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }))?;

        let counter_clone = counter.clone();
        property.subscribe(Arc::new(move |old, new| {
            println!("   Observer 2: {} -> {}", old, new);
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }))?;

        println!("   Observers registered: {}", property.observer_count());

        // Use the property
        property.set(42)?;
        println!("   Notifications sent: {}\n", counter.load(Ordering::SeqCst));

        // Graceful shutdown with timeout
        println!("   Initiating shutdown...");
        let report = property.shutdown_with_timeout(Duration::from_secs(5)).await?;
        
        println!("   Shutdown complete!");
        println!("   - Observers cleared: {}", report.observers_cleared);
        println!("   - Duration: {:?}", report.shutdown_duration);
        println!("   - Completed within timeout: {}", report.completed_within_timeout);
        println!("   - Diagnostic: {}\n", report.diagnostic_info());
    }

    // Example 2: Shutdown with async observers
    println!("2. Shutdown with Async Observers:");
    {
        let property = ObservableProperty::new("initial");
        let counter = Arc::new(AtomicUsize::new(0));

        // Add async observers that simulate I/O operations
        let counter_clone = counter.clone();
        property.subscribe_async(move |old, new| {
            let counter = counter_clone.clone();
            async move {
                println!("   Async observer 1 processing: '{}' -> '{}'", old, new);
                sleep(Duration::from_millis(100)).await;
                counter.fetch_add(1, Ordering::SeqCst);
                println!("   Async observer 1 complete");
            }
        })?;

        let counter_clone = counter.clone();
        property.subscribe_async(move |old, new| {
            let counter = counter_clone.clone();
            async move {
                println!("   Async observer 2 processing: '{}' -> '{}'", old, new);
                sleep(Duration::from_millis(150)).await;
                counter.fetch_add(1, Ordering::SeqCst);
                println!("   Async observer 2 complete");
            }
        })?;

        // Trigger async operations
        property.set_async("processing").await?;
        
        // Give time for async operations to complete
        sleep(Duration::from_millis(200)).await;
        println!("   Async operations completed: {}\n", counter.load(Ordering::SeqCst));

        // Shutdown
        println!("   Initiating shutdown with grace period...");
        let report = property.shutdown_with_timeout(Duration::from_secs(2)).await?;
        println!("   Shutdown report: {}\n", report.diagnostic_info());
    }

    // Example 3: Shutdown with mixed observer types
    println!("3. Shutdown with Mixed Observer Types:");
    {
        let property = ObservableProperty::new(0);
        let sync_counter = Arc::new(AtomicUsize::new(0));
        let async_counter = Arc::new(AtomicUsize::new(0));
        let filtered_counter = Arc::new(AtomicUsize::new(0));

        // Synchronous observer
        let sync_clone = sync_counter.clone();
        property.subscribe(Arc::new(move |_, _| {
            sync_clone.fetch_add(1, Ordering::SeqCst);
        }))?;

        // Async observer
        let async_clone = async_counter.clone();
        property.subscribe_async(move |_, _| {
            let counter = async_clone.clone();
            async move {
                sleep(Duration::from_millis(50)).await;
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })?;

        // Filtered observer (only even numbers)
        let filtered_clone = filtered_counter.clone();
        property.subscribe_filtered(
            Arc::new(move |_, _| {
                filtered_clone.fetch_add(1, Ordering::SeqCst);
            }),
            |_, new| new % 2 == 0
        )?;

        println!("   Total observers: {}", property.observer_count());

        // Trigger observers with various values
        property.set(1)?;  // filtered won't trigger
        property.set(2)?;  // all trigger
        sleep(Duration::from_millis(100)).await;

        println!("   Sync notifications: {}", sync_counter.load(Ordering::SeqCst));
        println!("   Async notifications: {}", async_counter.load(Ordering::SeqCst));
        println!("   Filtered notifications: {}", filtered_counter.load(Ordering::SeqCst));

        // Shutdown all observers
        let report = property.shutdown_with_timeout(Duration::from_secs(1)).await?;
        println!("   All {} observers shutdown successfully\n", report.observers_cleared);
    }

    // Example 4: Comparing shutdown methods
    println!("4. Comparing shutdown() vs shutdown_with_timeout():");
    {
        // Regular shutdown (fast, no report)
        let property1 = ObservableProperty::new(100);
        property1.subscribe(Arc::new(|_, _| {}))?;
        property1.subscribe(Arc::new(|_, _| {}))?;
        
        println!("   Using shutdown():");
        property1.shutdown()?;
        println!("   - Fast cleanup, no waiting");
        println!("   - No diagnostic report");
        println!("   - Observers cleared: {}\n", property1.observer_count());

        // Shutdown with timeout (provides report and grace period)
        let property2 = ObservableProperty::new(200);
        property2.subscribe(Arc::new(|_, _| {}))?;
        property2.subscribe(Arc::new(|_, _| {}))?;
        
        println!("   Using shutdown_with_timeout():");
        let report = property2.shutdown_with_timeout(Duration::from_secs(5)).await?;
        println!("   - Waits for grace period");
        println!("   - Provides diagnostic report");
        println!("   - Observers cleared: {}", report.observers_cleared);
        println!("   - Duration: {:?}\n", report.shutdown_duration);
    }

    // Example 5: Production shutdown pattern
    println!("5. Production Shutdown Pattern:");
    {
        let property = ObservableProperty::new("active");

        // Simulate application observers
        for i in 1..=5 {
            property.subscribe(Arc::new(move |_, new| {
                println!("   Worker {} notified: {}", i, new);
            }))?;
        }

        // Application running...
        property.set("processing")?;

        // Application shutdown signal received
        println!("\n   === Application shutdown initiated ===");
        
        let shutdown_timeout = Duration::from_secs(30);
        println!("   Maximum shutdown timeout: {:?}", shutdown_timeout);
        
        let report = property.shutdown_with_timeout(shutdown_timeout).await?;
        
        if report.completed_within_timeout {
            println!("   ✓ Graceful shutdown completed successfully");
        } else {
            println!("   ⚠ Shutdown timeout exceeded, forced termination");
        }
        
        println!("   Final report: {}", report.diagnostic_info());
        
        // Log for monitoring/alerting
        if report.shutdown_duration > Duration::from_secs(10) {
            println!("   ⚠ WARNING: Shutdown took longer than expected");
            println!("   Consider investigating slow observers");
        }
    }

    println!("\n=== Production Recommendations ===");
    println!("• Use shutdown_with_timeout() in production for observability");
    println!("• Set appropriate timeout based on your application needs");
    println!("• Log shutdown reports for monitoring and debugging");
    println!("• Monitor shutdown duration to detect performance issues");
    println!("• Use graceful shutdown during application lifecycle events");
    println!("• Consider implementing health checks with shutdown status");

    println!("\n=== Benefits of Graceful Shutdown ===");
    println!("✓ Provides diagnostic information about cleanup process");
    println!("✓ Allows pending async operations time to complete");
    println!("✓ Generates metrics for monitoring and alerting");
    println!("✓ Helps identify slow or stuck observers");
    println!("✓ Supports proper application lifecycle management");
    println!("✓ Enables clean resource cleanup in production systems");

    Ok(())
}
