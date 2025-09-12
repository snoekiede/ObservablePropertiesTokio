use observable_property_tokio::ObservableProperty;
use std::sync::Arc;

// Define a complex data type with multiple fields
#[derive(Clone, Debug)]
struct User {
    id: u64,
    name: String,
    email: String,
    active: bool,
    login_count: u32,
}

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    println!("=== Complex Data Type Example ===");
    println!("This example demonstrates using ObservableProperty with complex data types");

    // Create an initial user
    let initial_user = User {
        id: 1,
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        active: true,
        login_count: 0,
    };

    // Create an observable property with the user
    let user_property = ObservableProperty::new(initial_user);

    // Print initial state
    println!("\nInitial user:");
    println!("{:#?}", user_property.get()?);

    // Subscribe to any changes to the user
    let _subscription = user_property.subscribe_with_token(Arc::new(|old_user, new_user| {
        println!("\nUser changed:");
        println!("Name: {} -> {}", old_user.name, new_user.name);
        println!("Email: {} -> {}", old_user.email, new_user.email);
        println!("Active: {} -> {}", old_user.active, new_user.active);
        println!("Login count: {} -> {}", old_user.login_count, new_user.login_count);
    }))?;

    // Create filtered subscriptions for specific field changes
    let _name_change_subscription = user_property.subscribe_filtered_with_token(
        Arc::new(|_, new_user| {
            println!("\n[NAME CHANGE ALERT] User name is now: {}", new_user.name);
        }),
        |old_user, new_user| old_user.name != new_user.name
    )?;

    let _status_change_subscription = user_property.subscribe_filtered_with_token(
        Arc::new(|_, new_user| {
            if new_user.active {
                println!("\n[STATUS ALERT] User is now active");
            } else {
                println!("\n[STATUS ALERT] User is now inactive");
            }
        }),
        |old_user, new_user| old_user.active != new_user.active
    )?;

    // Update the user (modifying multiple fields)
    println!("\nUpdating user...");
    user_property.update(|mut user| {
        user.name = "Alice Smith".to_string();
        user.login_count += 1;
        user
    })?;

    // Update just the email
    println!("\nUpdating email only...");
    user_property.update(|mut user| {
        user.email = "alice.smith@example.com".to_string();
        user
    })?;

    // Deactivate the user
    println!("\nDeactivating user...");
    user_property.update(|mut user| {
        user.active = false;
        user
    })?;

    // Display the final state
    println!("\nFinal user state:");
    println!("{:#?}", user_property.get()?);

    Ok(())
}
