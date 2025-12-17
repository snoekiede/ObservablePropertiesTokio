use observable_property_tokio::{ObservableProperty, PropertyError};
use std::sync::Arc;

/// This example demonstrates the enhanced error diagnostic capabilities
/// that help with production monitoring and debugging.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Observable Property Error Diagnostics Example ===\n");

    // Create a property
    let property = ObservableProperty::new(0);

    // Example 1: Demonstrating error creation with diagnostic context
    println!("1. Error Helper Functions:");
    let read_error = PropertyError::read_lock_error("get_value", "failed to acquire read lock");
    println!("   Error: {}", read_error);
    println!("   Diagnostic: {}\n", read_error.diagnostic_info());

    // Example 2: Capacity exceeded error (useful for rate limiting)
    println!("2. Capacity Exceeded Error:");
    let capacity_error = PropertyError::CapacityExceeded {
        current: 150,
        max: 100,
        resource: "observers".to_string(),
    };
    println!("   Error: {}", capacity_error);
    println!("   Diagnostic: {}\n", capacity_error.diagnostic_info());

    // Example 3: Operation timeout error (useful for SLA monitoring)
    println!("3. Operation Timeout Error:");
    let timeout_error = PropertyError::OperationTimeout {
        operation: "notify_observers".to_string(),
        elapsed_ms: 5500,
        threshold_ms: 5000,
    };
    println!("   Error: {}", timeout_error);
    println!("   Diagnostic: {}\n", timeout_error.diagnostic_info());

    // Example 4: Observer panic error (useful for debugging observer issues)
    println!("4. Observer Panic Error:");
    let panic_error = PropertyError::observer_panic(
        observable_property_tokio::ObserverId::from(42),
        "observer function panicked with: division by zero"
    );
    println!("   Error: {}", panic_error);
    println!("   Diagnostic: {}\n", panic_error.diagnostic_info());

    // Example 5: Lock poisoned error (critical for concurrency debugging)
    println!("5. Lock Poisoned Error:");
    let poisoned_error = PropertyError::lock_poisoned(
        "set_value",
        "inner property lock was poisoned by previous panic"
    );
    println!("   Error: {}", poisoned_error);
    println!("   Diagnostic: {}\n", poisoned_error.diagnostic_info());

    // Example 6: Practical usage with error handling
    println!("6. Practical Usage with Error Handling:");
    
    // Subscribe an observer that will work normally
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter_clone = counter.clone();
    
    property.subscribe(Arc::new(move |old, new| {
        println!("   Observer triggered: {} -> {}", old, new);
        counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }))?;

    // Normal operation
    match property.set(42) {
        Ok(_) => println!("   ✓ Successfully set value to 42"),
        Err(e) => {
            eprintln!("   ✗ Error occurred: {}", e);
            eprintln!("   Diagnostic info: {}", e.diagnostic_info());
            // In production, you would log this to your monitoring system
        }
    }

    // Example 7: Demonstrating observer not found error
    println!("\n7. Observer Not Found Error:");
    let fake_id = observable_property_tokio::ObserverId::from(999);
    match property.unsubscribe(fake_id) {
        Ok(_) => println!("   Observer removed"),
        Err(e) => {
            println!("   Expected error: {}", e);
            println!("   Diagnostic: {}", e.diagnostic_info());
        }
    }

    // Example 8: Shutdown in progress error (useful for graceful shutdown)
    println!("\n8. Shutdown In Progress Error:");
    let shutdown_error = PropertyError::ShutdownInProgress;
    println!("   Error: {}", shutdown_error);
    println!("   Diagnostic: {}\n", shutdown_error.diagnostic_info());

    println!("=== Integration with Logging ===");
    println!("In production, you would typically use the diagnostic_info() method like this:");
    println!(r#"
    match property.set(value) {{
        Ok(_) => {{ /* success */ }}
        Err(e) => {{
            // Log to your monitoring system
            log::error!("Property operation failed: {{}}", e.diagnostic_info());
            
            // Or send to a metrics system
            metrics::increment_counter!("property_errors", "type" => error_type(&e));
            
            // Or include in structured logging
            tracing::error!(
                diagnostic = %e.diagnostic_info(),
                error = %e,
                "Property operation failed"
            );
        }}
    }}
    "#);

    println!("\n=== Benefits of Diagnostic Information ===");
    println!("• Timestamps help correlate errors with other system events");
    println!("• Operation context shows exactly what was being attempted");
    println!("• Resource metrics (like utilization %) aid in capacity planning");
    println!("• Structured format makes errors easy to parse and aggregate");
    println!("• Performance metrics help identify bottlenecks and SLA violations");

    Ok(())
}
