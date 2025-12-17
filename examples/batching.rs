use observable_property_tokio::{BatchedProperty, BatchConfig, ObservableProperty, PropertyError};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> Result<(), PropertyError> {
    println!("=== Observable Property Batching Examples ===\n");

    example_1_basic_batching().await?;
    example_2_comparison_with_unbatched().await?;
    example_3_custom_batch_interval().await?;
    example_4_immediate_updates().await?;
    example_5_manual_flush().await?;
    example_6_high_frequency_updates().await?;
    example_7_batch_configuration().await?;
    example_8_async_observers().await?;
    example_9_production_use_case().await?;
    example_10_performance_comparison().await?;

    Ok(())
}

/// Example 1: Basic Batching
/// Demonstrates how batching reduces the number of observer notifications
async fn example_1_basic_batching() -> Result<(), PropertyError> {
    println!("1. Basic Batching");
    println!("   Queue multiple updates rapidly, get notified once per batch interval\n");

    let property = BatchedProperty::new(0);
    let notification_count = Arc::new(AtomicUsize::new(0));

    // Subscribe to batched updates
    property.subscribe(Arc::new({
        let counter = notification_count.clone();
        move |old, new| {
            counter.fetch_add(1, Ordering::SeqCst);
            println!("   Batched notification: {} -> {}", old, new);
        }
    }))?;

    // Queue 100 updates rapidly
    println!("   Queueing 100 updates...");
    for i in 1..=100 {
        property.queue_update(i)?;
    }

    // Wait for batch to flush (default is 100ms)
    tokio::time::sleep(Duration::from_millis(150)).await;

    println!("   Total notifications: {}", notification_count.load(Ordering::SeqCst));
    println!("   Final value: {}\n", property.get()?);

    Ok(())
}

/// Example 2: Comparison with Unbatched Property
/// Shows the difference in notification count between batched and unbatched
async fn example_2_comparison_with_unbatched() -> Result<(), PropertyError> {
    println!("2. Comparison: Batched vs Unbatched");
    println!("   Compare notification counts for the same updates\n");

    // Unbatched property
    let unbatched = ObservableProperty::new(0);
    let unbatched_count = Arc::new(AtomicUsize::new(0));
    
    unbatched.subscribe(Arc::new({
        let counter = unbatched_count.clone();
        move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
    }))?;

    // Batched property
    let batched = BatchedProperty::new(0);
    let batched_count = Arc::new(AtomicUsize::new(0));
    
    batched.subscribe(Arc::new({
        let counter = batched_count.clone();
        move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
    }))?;

    // Send 50 updates to both
    println!("   Sending 50 updates to both properties...");
    for i in 1..=50 {
        unbatched.set(i)?;
        batched.queue_update(i)?;
    }

    // Wait for batch to flush
    tokio::time::sleep(Duration::from_millis(150)).await;

    println!("   Unbatched notifications: {}", unbatched_count.load(Ordering::SeqCst));
    println!("   Batched notifications: {}", batched_count.load(Ordering::SeqCst));
    println!("   Reduction: {}%\n", 
        (1.0 - batched_count.load(Ordering::SeqCst) as f64 / unbatched_count.load(Ordering::SeqCst) as f64) * 100.0);

    Ok(())
}

/// Example 3: Custom Batch Interval
/// Demonstrates using different batch intervals
async fn example_3_custom_batch_interval() -> Result<(), PropertyError> {
    println!("3. Custom Batch Interval");
    println!("   Use a shorter batch interval for lower latency\n");

    let config = BatchConfig {
        batch_interval: Duration::from_millis(50),
    };

    let property = BatchedProperty::new_with_config(0, config);
    let notification_count = Arc::new(AtomicUsize::new(0));

    property.subscribe(Arc::new({
        let counter = notification_count.clone();
        move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
    }))?;

    println!("   Queueing updates...");
    for i in 1..=20 {
        property.queue_update(i)?;
    }

    // Wait for the shorter batch interval
    tokio::time::sleep(Duration::from_millis(75)).await;

    println!("   Notifications after 75ms: {}", notification_count.load(Ordering::SeqCst));
    println!("   (With 50ms batch interval, notification arrives faster)\n");

    Ok(())
}

/// Example 4: Immediate Updates
/// Shows how to bypass batching when needed
async fn example_4_immediate_updates() -> Result<(), PropertyError> {
    println!("4. Immediate Updates");
    println!("   Bypass batching for critical updates\n");

    let property = BatchedProperty::new(0);
    let notification_count = Arc::new(AtomicUsize::new(0));
    let last_value = Arc::new(parking_lot::RwLock::new(0));

    property.subscribe(Arc::new({
        let counter = notification_count.clone();
        let last_value = last_value.clone();
        move |_, new| {
            counter.fetch_add(1, Ordering::SeqCst);
            *last_value.write() = *new;
            println!("   Notification: value = {}", new);
        }
    }))?;

    // Queue some updates
    println!("   Queueing non-critical updates...");
    property.queue_update(1)?;
    property.queue_update(2)?;

    // Set immediately for critical update
    println!("   Sending critical update immediately...");
    property.set_immediate(999)?;

    println!("   Immediate value: {}", property.get()?);
    println!("   Notifications so far: {}\n", notification_count.load(Ordering::SeqCst));

    // Wait for potential batch
    tokio::time::sleep(Duration::from_millis(150)).await;

    println!("   Total notifications: {}\n", notification_count.load(Ordering::SeqCst));

    Ok(())
}

/// Example 5: Manual Flush
/// Demonstrates manual flushing of batched updates
async fn example_5_manual_flush() -> Result<(), PropertyError> {
    println!("5. Manual Flush");
    println!("   Force immediate processing of batched updates\n");

    let property = BatchedProperty::new(0);
    let notification_count = Arc::new(AtomicUsize::new(0));

    property.subscribe(Arc::new({
        let counter = notification_count.clone();
        move |old, new| {
            counter.fetch_add(1, Ordering::SeqCst);
            println!("   Notification: {} -> {}", old, new);
        }
    }))?;

    // Queue updates
    println!("   Queueing updates...");
    property.queue_update(42)?;

    // Flush immediately instead of waiting for batch interval
    println!("   Flushing immediately...");
    property.flush().await?;

    println!("   Notifications: {}", notification_count.load(Ordering::SeqCst));
    println!("   Value: {}\n", property.get()?);

    Ok(())
}

/// Example 6: High-Frequency Updates
/// Simulates a real-world high-frequency update scenario
async fn example_6_high_frequency_updates() -> Result<(), PropertyError> {
    println!("6. High-Frequency Updates");
    println!("   Simulate sensor data or real-time metrics\n");

    let config = BatchConfig {
        batch_interval: Duration::from_millis(100),
    };

    let property = BatchedProperty::new_with_config(0.0, config);
    let notification_count = Arc::new(AtomicUsize::new(0));

    property.subscribe(Arc::new({
        let counter = notification_count.clone();
        move |_, new| {
            counter.fetch_add(1, Ordering::SeqCst);
            println!("   Sensor reading: {:.2}", new);
        }
    }))?;

    // Simulate 500 sensor readings over 250ms
    println!("   Simulating 500 sensor readings...");
    let start = Instant::now();
    
    for i in 0..500 {
        let reading = 20.0 + (i as f64 * 0.1);
        property.queue_update(reading)?;
        
        // Small delay to simulate real sensor timing
        if i % 100 == 0 {
            tokio::time::sleep(Duration::from_micros(500)).await;
        }
    }

    let elapsed = start.elapsed();
    
    // Wait for final batch
    tokio::time::sleep(Duration::from_millis(150)).await;

    println!("   Generated 500 readings in {:?}", elapsed);
    println!("   Observer notifications: {}", notification_count.load(Ordering::SeqCst));
    println!("   Notification reduction: ~{}x\n", 500 / notification_count.load(Ordering::SeqCst).max(1));

    Ok(())
}

/// Example 7: Batch Configuration Strategies
/// Shows different configuration strategies for different use cases
async fn example_7_batch_configuration() -> Result<(), PropertyError> {
    println!("7. Batch Configuration Strategies\n");

    // Low-latency configuration (50ms)
    println!("   Low-latency config (50ms interval):");
    println!("   - Best for: Interactive UI updates");
    println!("   - Trade-off: More notifications, lower latency\n");

    // Balanced configuration (100ms - default)
    println!("   Balanced config (100ms interval):");
    println!("   - Best for: Most applications");
    println!("   - Trade-off: Good balance of latency and efficiency\n");

    // High-throughput configuration (200ms)
    println!("   High-throughput config (200ms interval):");
    println!("   - Best for: Background data processing");
    println!("   - Trade-off: Fewer notifications, higher latency\n");

    // Aggressive batching (500ms)
    println!("   Aggressive batching (500ms interval):");
    println!("   - Best for: Analytics, logging, non-critical updates");
    println!("   - Trade-off: Minimal notifications, significant latency\n");

    Ok(())
}

/// Example 8: Async Observers with Batching
/// Demonstrates batching with async observers
async fn example_8_async_observers() -> Result<(), PropertyError> {
    println!("8. Async Observers with Batching");
    println!("   Combine batching with async processing\n");

    let property = BatchedProperty::new(0);
    let notification_count = Arc::new(AtomicUsize::new(0));

    property.subscribe_async({
        let counter = notification_count.clone();
        move |_, new| {
            let counter = counter.clone();
            async move {
                // Simulate async processing (e.g., API call, database write)
                tokio::time::sleep(Duration::from_millis(10)).await;
                counter.fetch_add(1, Ordering::SeqCst);
                println!("   Async processed: {}", new);
            }
        }
    })?;

    println!("   Queueing 50 updates...");
    for i in 1..=50 {
        property.queue_update(i)?;
    }

    // Wait for batch and async processing
    tokio::time::sleep(Duration::from_millis(200)).await;

    println!("   Async notifications: {}", notification_count.load(Ordering::SeqCst));
    println!("   (Batching reduces async task spawning overhead)\n");

    Ok(())
}

/// Example 9: Production Use Case - Real-Time Dashboard
/// Simulates a real-time dashboard with multiple metrics
async fn example_9_production_use_case() -> Result<(), PropertyError> {
    println!("9. Production Use Case: Real-Time Dashboard");
    println!("   Multiple metrics updating at different rates\n");

    let config = BatchConfig {
        batch_interval: Duration::from_millis(100),
    };

    // CPU usage metric
    let cpu_metric = BatchedProperty::new_with_config(0.0, config.clone());
    cpu_metric.subscribe(Arc::new(|_, new| {
        println!("   📊 CPU: {:.1}%", new);
    }))?;

    // Memory usage metric
    let memory_metric = BatchedProperty::new_with_config(0.0, config.clone());
    memory_metric.subscribe(Arc::new(|_, new| {
        println!("   💾 Memory: {:.1}MB", new);
    }))?;

    // Network throughput metric
    let network_metric = BatchedProperty::new_with_config(0.0, config);
    network_metric.subscribe(Arc::new(|_, new| {
        println!("   🌐 Network: {:.1}Mbps", new);
    }))?;

    println!("   Simulating 200 metric updates over 250ms...\n");

    // Simulate metrics updating at high frequency
    for i in 0..200 {
        cpu_metric.queue_update(50.0 + (i as f64 % 30.0))?;
        memory_metric.queue_update(1024.0 + (i as f64 * 2.0))?;
        network_metric.queue_update(100.0 + (i as f64 % 50.0))?;
        
        if i % 50 == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    // Wait for final batch
    tokio::time::sleep(Duration::from_millis(200)).await;

    println!("\n   Dashboard updated efficiently with batching!");
    println!("   (Without batching: 600 notifications, With batching: ~6-9 notifications)\n");

    Ok(())
}

/// Example 10: Performance Comparison
/// Measures the performance impact of batching
async fn example_10_performance_comparison() -> Result<(), PropertyError> {
    println!("10. Performance Comparison");
    println!("    Measuring overhead of batched vs unbatched updates\n");

    let update_count = 1000;

    // Measure unbatched performance
    let unbatched = ObservableProperty::new(0);
    let unbatched_notifications = Arc::new(AtomicUsize::new(0));
    
    unbatched.subscribe(Arc::new({
        let counter = unbatched_notifications.clone();
        move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
    }))?;

    let start = Instant::now();
    for i in 0..update_count {
        unbatched.set(i)?;
    }
    let unbatched_time = start.elapsed();

    // Measure batched performance
    let batched = BatchedProperty::new(0);
    let batched_notifications = Arc::new(AtomicUsize::new(0));
    
    batched.subscribe(Arc::new({
        let counter = batched_notifications.clone();
        move |_, _| { counter.fetch_add(1, Ordering::SeqCst); }
    }))?;

    let start = Instant::now();
    for i in 0..update_count {
        batched.queue_update(i)?;
    }
    let batched_time = start.elapsed();

    // Wait for batches to complete
    tokio::time::sleep(Duration::from_millis(200)).await;

    println!("    Unbatched:");
    println!("      - Time: {:?}", unbatched_time);
    println!("      - Notifications: {}", unbatched_notifications.load(Ordering::SeqCst));
    
    println!("\n    Batched:");
    println!("      - Time: {:?}", batched_time);
    println!("      - Notifications: {}", batched_notifications.load(Ordering::SeqCst));
    
    println!("\n    Improvement:");
    println!("      - Notification reduction: {}x", 
        unbatched_notifications.load(Ordering::SeqCst) / batched_notifications.load(Ordering::SeqCst).max(1));
    
    if batched_time < unbatched_time {
        let speedup = unbatched_time.as_micros() as f64 / batched_time.as_micros() as f64;
        println!("      - Queue speedup: {:.2}x faster", speedup);
    }

    println!("\n=== All Examples Completed ===\n");

    Ok(())
}
