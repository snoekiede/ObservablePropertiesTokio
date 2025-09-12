use observable_property_tokio::ObservableProperty;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

// A slightly expensive data structure to demonstrate benefits of get_ref()
#[derive(Clone, Debug)]
struct LargeDataSet {
    id: String,
    data: Vec<f64>, // Simulating a large dataset
    timestamp: u64,
}

impl LargeDataSet {
    fn new(id: &str, size: usize) -> Self {
        // Generate a dataset with some values
        let data = (0..size).map(|i| i as f64 / 100.0).collect();

        Self {
            id: id.to_string(),
            data,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    // Calculate average without cloning
    fn average(&self) -> f64 {
        if self.data.is_empty() {
            return 0.0;
        }
        self.data.iter().sum::<f64>() / self.data.len() as f64
    }
}

#[tokio::main]
async fn main() -> Result<(), observable_property_tokio::PropertyError> {
    println!("=== Property References and Async Filtered Subscriptions ===");
    println!("This example demonstrates:");
    println!(" - Using get_ref() to access data without cloning");
    println!(" - Async filtered subscriptions for selective notification");

    // Create a "large" dataset as our property value
    let dataset = LargeDataSet::new("dataset-1", 10_000);
    let property = ObservableProperty::new(dataset);

    // 1. Using get_ref() to access data without cloning
    println!("\n--- Using get_ref() ---");
    {
        // Get a reference to the current value (doesn't clone the large Vec)
        let dataset_ref = property.get_ref();

        println!("Dataset ID: {}", dataset_ref.id);
        println!("Timestamp: {}", dataset_ref.timestamp);
        println!("Dataset size: {} elements", dataset_ref.data.len());
        println!("Average value: {:.4}", dataset_ref.average());

        // Reference is automatically dropped at the end of this scope
    }

    // 2. Set up an async filtered subscription
    println!("\n--- Async Filtered Subscription ---");
    println!("Subscribing to dataset changes where average value increases...");

    // Subscribe with async handler that is filtered - only runs when avg increases
    let _subscription = property.subscribe_async_filtered_with_token(
        |old_data, new_data| async move {
            // Simulate async processing
            sleep(Duration::from_millis(20)).await;

            let old_avg = old_data.average();
            let new_avg = new_data.average();
            println!("ASYNC NOTIFICATION: Average changed from {:.4} to {:.4}", old_avg, new_avg);
        },
        // Filter: only notify when the average increases
        |old_data, new_data| new_data.average() > old_data.average()
    )?;

    // Update the dataset multiple times
    println!("\nUpdating dataset with lower average (should NOT trigger observer)...");
    property.update(|mut dataset| {
        dataset.id = "dataset-2".to_string();
        // Replace with values that will decrease the average
        dataset.data = (0..10_000).map(|i| (i as f64 / 200.0)).collect();
        dataset
    })?;

    // Wait a bit to allow for any async processing
    sleep(Duration::from_millis(50)).await;

    println!("\nUpdating dataset with higher average (should trigger observer)...");
    property.update(|mut dataset| {
        dataset.id = "dataset-3".to_string();
        // Replace with values that will increase the average
        dataset.data = (0..10_000).map(|i| (i as f64 / 50.0)).collect();
        dataset
    })?;

    // Wait for async processing
    sleep(Duration::from_millis(50)).await;

    // One more update with mixed changes
    println!("\nFinal update with mixed changes (should trigger observer)...");
    property.update_async(|mut dataset| {
        dataset.id = "dataset-4".to_string();
        dataset.timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Another increase in average
        dataset.data = (0..10_000).map(|i| (i as f64 / 25.0)).collect();
        dataset
    }).await?;

    // Wait for async processing
    sleep(Duration::from_millis(50)).await;

    // Show final state using get_ref() again
    let final_ref = property.get_ref();
    println!("\nFinal dataset:");
    println!("ID: {}", final_ref.id);
    println!("Timestamp: {}", final_ref.timestamp);
    println!("Average: {:.4}", final_ref.average());

    Ok(())
}
