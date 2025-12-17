use observable_property_tokio::{ObservableProperty, PropertyConfig};
use std::sync::Arc;

/// This example demonstrates backpressure and rate limiting features
/// to prevent resource exhaustion with configurable limits.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Observable Property Backpressure & Rate Limiting Example ===\n");

    // Example 1: Default configuration
    println!("1. Default Configuration:");
    let _property_default = ObservableProperty::new(0);
    println!("   Default max_observers: 1000");
    println!("   Default max_pending_notifications: 100");
    println!("   Default observer_timeout_ms: 5000ms\n");

    // Example 2: Custom configuration with limits
    println!("2. Custom Configuration with Low Limits:");
    let config = PropertyConfig {
        max_observers: 5,
        max_pending_notifications: 10,
        observer_timeout_ms: 3000,
        max_concurrent_async_tasks: 100,
    };
    
    let property = ObservableProperty::new_with_config(0, config);
    println!("   Custom max_observers: 5");
    println!("   Current observer count: {}\n", property.observer_count());

    // Example 3: Adding observers up to the limit
    println!("3. Adding Observers Up To Limit:");
    for i in 1..=5 {
        match property.subscribe(Arc::new(move |old, new| {
            println!("   Observer {} triggered: {} -> {}", i, old, new);
        })) {
            Ok(id) => println!("   ✓ Observer {} subscribed (ID: {})", i, id),
            Err(e) => println!("   ✗ Failed to subscribe observer {}: {}", i, e),
        }
    }
    println!("   Total observers: {}\n", property.observer_count());

    // Example 4: Attempting to exceed the limit
    println!("4. Attempting to Exceed Observer Limit:");
    match property.subscribe(Arc::new(|_, _| {})) {
        Ok(_) => println!("   Unexpected: subscription succeeded"),
        Err(e) => {
            println!("   ✗ Expected error: {}", e);
            println!("   Diagnostic: {}\n", e.diagnostic_info());
        }
    }

    // Example 5: Triggering observers
    println!("5. Triggering Observers (all 5 should respond):");
    property.set(42)?;
    println!();

    // Example 6: Freeing up capacity by unsubscribing
    println!("6. Freeing Up Capacity:");
    println!("   Current observers: {}", property.observer_count());
    
    // Get an observer ID from earlier subscriptions - let's remove the first one
    // In real usage, you'd store the ID when you subscribe
    // For this demo, we'll create a new property to demonstrate the pattern
    let demo_property = ObservableProperty::new_with_config(
        0,
        PropertyConfig {
            max_observers: 3,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 100,
        },
    );
    
    let id1 = demo_property.subscribe(Arc::new(|_, _| {}))?;
    let _id2 = demo_property.subscribe(Arc::new(|_, _| {}))?;
    let _id3 = demo_property.subscribe(Arc::new(|_, _| {}))?;
    
    println!("   Current observers before unsubscribe: {}", demo_property.observer_count());
    
    // This will fail because we're at capacity
    match demo_property.subscribe(Arc::new(|_, _| {})) {
        Ok(_) => println!("   Unexpected: subscription succeeded"),
        Err(_) => println!("   ✗ Cannot add more (at capacity)"),
    }
    
    // Unsubscribe to free up space
    demo_property.unsubscribe(id1)?;
    println!("   Current observers after unsubscribe: {}", demo_property.observer_count());
    
    // Now we can add another observer
    match demo_property.subscribe(Arc::new(|old, new| {
        println!("   New observer after freeing capacity triggered: {} -> {}", old, new);
    })) {
        Ok(_) => println!("   ✓ Successfully added observer after freeing capacity"),
        Err(e) => println!("   ✗ Failed: {}", e),
    }
    
    // Test it
    demo_property.set(999)?;
    println!();

    // Example 7: Async observers also respect limits
    println!("7. Async Observers Also Respect Limits:");
    let async_property = ObservableProperty::new_with_config(
        "initial",
        PropertyConfig {
            max_observers: 2,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 100,
        },
    );

    async_property.subscribe_async(|old, new| async move {
        println!("   Async observer 1: '{}' -> '{}'", old, new);
    })?;

    async_property.subscribe_async(|old, new| async move {
        println!("   Async observer 2: '{}' -> '{}'", old, new);
    })?;

    println!("   Current async observers: {}", async_property.observer_count());

    match async_property.subscribe_async(|_, _| async move {}) {
        Ok(_) => println!("   Unexpected: subscription succeeded"),
        Err(e) => {
            println!("   ✗ Expected error: {}", e);
            println!("   Diagnostic: {}", e.diagnostic_info());
        }
    }
    println!();

    // Example 8: Filtered observers also respect limits
    println!("8. Filtered Observers Respect Limits:");
    let filtered_property = ObservableProperty::new_with_config(
        0,
        PropertyConfig {
            max_observers: 2,
            max_pending_notifications: 100,
            observer_timeout_ms: 5000,
            max_concurrent_async_tasks: 100,
        },
    );

    filtered_property.subscribe_filtered(
        Arc::new(|_, new| println!("   Filtered observer 1: even value {}", new)),
        |_, new| new % 2 == 0
    )?;

    filtered_property.subscribe_filtered(
        Arc::new(|_, new| println!("   Filtered observer 2: odd value {}", new)),
        |_, new| new % 2 == 1
    )?;

    match filtered_property.subscribe_filtered(Arc::new(|_, _| {}), |_, _| true) {
        Ok(_) => println!("   Unexpected: subscription succeeded"),
        Err(e) => println!("   ✗ Expected error: {}", e),
    }
    println!();

    // Example 9: Error handling with match patterns
    println!("9. Handling Capacity Errors in Production Code:");
    println!("   Example pattern for production use:");
    println!(r#"
   match property.subscribe(observer) {{
       Ok(id) => {{
           log::info!("Observer {{}} subscribed successfully", id);
           // Store id for later unsubscribe
       }}
       Err(PropertyError::CapacityExceeded {{ current, max, .. }}) => {{
           log::warn!("Observer limit reached: {{}}/{{}}", current, max);
           // Handle gracefully - maybe implement a queue or reject request
       }}
       Err(e) => {{
           log::error!("Unexpected error: {{}}", e.diagnostic_info());
       }}
   }}
   "#);

    // Example 10: Production recommendations
    println!("10. Production Recommendations:");
    println!("   • Set max_observers based on your expected load");
    println!("   • Monitor CapacityExceeded errors to tune limits");
    println!("   • Use unsubscribe() or subscription tokens for cleanup");
    println!("   • Consider implementing a queue for excess subscriptions");
    println!("   • Log diagnostic_info() for all capacity errors");
    println!("   • Test limits under peak load conditions");
    println!();

    println!("=== Benefits of Backpressure ===");
    println!("• Prevents memory exhaustion from unlimited observers");
    println!("• Provides early failure before system resources are depleted");
    println!("• Enables capacity planning with predictable resource usage");
    println!("• Supports graceful degradation under high load");
    println!("• Makes resource limits explicit and configurable");

    Ok(())
}
