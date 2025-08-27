use observable_property_tokio::ObservableProperty;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    println!("=== Filtered Observers Example ===");

    let property = ObservableProperty::new(0);
    println!("Initial value: {}", property.get()?);

    // Observer that only triggers when value increases
    let increase_observer = property.subscribe_filtered(
        Arc::new(|old, new| {
            println!("✅ Value increased: {} -> {}", old, new);
        }),
        |old, new| new > old
    )?;

    // Observer that only triggers when value decreases
    let decrease_observer = property.subscribe_filtered(
        Arc::new(|old, new| {
            println!("⬇️ Value decreased: {} -> {}", old, new);
        }),
        |old, new| new < old
    )?;

    // Observer that only triggers on even numbers
    let even_observer = property.subscribe_filtered(
        Arc::new(|_old, new| {
            println!("🎯 New value is even: {}", new);
        }),
        |_old, new| new % 2 == 0
    )?;

    println!("\n--- Testing filtered observers ---");

    println!("\nSetting to 10 (increase + even):");
    property.set(10)?;

    println!("\nSetting to 5 (decrease + odd):");
    property.set(5)?;

    println!("\nSetting to 15 (increase + odd):");
    property.set(15)?;

    println!("\nSetting to 8 (decrease + even):");
    property.set(8)?;

    // Clean up
    property.unsubscribe(increase_observer)?;
    property.unsubscribe(decrease_observer)?;
    property.unsubscribe(even_observer)?;

    println!("\nAll observers unsubscribed.");

    Ok(())
}
