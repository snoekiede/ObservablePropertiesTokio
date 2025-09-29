use observable_property_tokio::ObservableProperty;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    println!("=== Property Mapping and Transformation Example ===");
    println!("This example demonstrates creating derived properties with transformations");

    // Create an original observable property with a number
    let original = ObservableProperty::new(42);
    println!("Original property value: {}", original.get()?);

    // Create a derived property that doubles the value
    let doubled = original.map(|value| value * 2)?;
    println!("Doubled property value: {}", doubled.get()?);

    // Create a derived property that converts the number to a string
    let as_string = original.map(|value| format!("The value is: {}", value))?;
    println!("String property value: {}", as_string.get()?);

    // Create another derived property that checks if the value is even
    let is_even = original.map(|value| value % 2 == 0)?;
    println!("Is even property value: {}", is_even.get()?);

    // When the original property changes, all derived properties update automatically
    println!("\nChanging original value to 15...");
    original.set(15)?;

    // Check that all derived properties updated
    println!("Original property value: {}", original.get()?);
    println!("Doubled property value: {}", doubled.get()?);
    println!("String property value: {}", as_string.get()?);
    println!("Is even property value: {}", is_even.get()?);

    // Chain transformations - create a property derived from the doubled property
    let doubled_plus_ten = doubled.map(|value| value + 10)?;
    println!("\nDoubled plus ten: {}", doubled_plus_ten.get()?);

    // Change original again and see all derived properties update
    println!("\nChanging original value to 100...");
    original.set(100)?;

    println!("Original property value: {}", original.get()?);
    println!("Doubled property value: {}", doubled.get()?);
    println!("String property value: {}", as_string.get()?);
    println!("Is even property value: {}", is_even.get()?);
    println!("Doubled plus ten: {}", doubled_plus_ten.get()?);

    Ok(())
}
