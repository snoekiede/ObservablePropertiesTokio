use observable_property_tokio::ObservableProperty;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    println!("=== Basic Observable Property Usage ===");

    // Create an observable property
    let property = ObservableProperty::new(42);
    println!("Initial value: {}", property.get()?);

    // Subscribe to changes
    let observer_id = property.subscribe(Arc::new(|old_value, new_value| {
        println!("Value changed from {} to {}", old_value, new_value);
    }))?;

    // Change the value (triggers observer)
    println!("\nSetting value to 100...");
    property.set(100)?;

    // For async notification (uses Tokio)
    println!("\nSetting value to 200 asynchronously...");
    property.set_async(200).await?;

    // Give time for async observers to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Unsubscribe when done
    property.unsubscribe(observer_id)?;

    println!("\nUnsubscribed observer. Setting value to 300 (should not trigger observer)...");
    property.set(300)?;

    println!("Final value: {}", property.get()?);

    Ok(())
}
