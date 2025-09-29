use observable_property_tokio::ObservableProperty;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    println!("=== Subscription Token Example ===");
    println!("This example demonstrates RAII-style automatic cleanup of subscriptions");

    // Create an observable property
    let property = ObservableProperty::new(0);

    // Create a counter to track observer invocations
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_display = counter.clone();

    println!("Initial counter value: {}", counter.load(Ordering::SeqCst));

    // Create a subscription in a nested scope
    {
        println!("\nCreating subscription in nested scope...");

        // Use subscribe_with_token to get an automatically managed subscription
        let _subscription = property.subscribe_with_token(Arc::new({
            let counter = counter.clone();
            move |old, new| {
                println!("Observer notified: {} -> {}", old, new);
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }))?;

        // Update property - observer should be triggered
        println!("Updating property within subscription scope...");
        property.set(1)?;
        property.set(2)?;

        println!("Counter after updates: {}", counter.load(Ordering::SeqCst));

        // Subscription will be automatically cleaned up when we exit this scope
        println!("Exiting scope - subscription will be dropped automatically");
    }

    // After exiting the scope, the subscription should be automatically unsubscribed
    println!("\nUpdating property after subscription scope has ended...");
    property.set(3)?;
    property.set(4)?;

    println!("Final counter value: {}", counter_display.load(Ordering::SeqCst));
    println!("Notice that the counter didn't increase after the subscription was dropped!");

    Ok(())
}
