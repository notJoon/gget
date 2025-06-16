use gget::parallel::{DownloadError, DownloadManager, DownloadTask, ProgressUpdate, RetryConfig};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_download_manager_basic() {
    let manager = DownloadManager::new(4);

    // Queue some tasks
    for i in 0..5 {
        let task = DownloadTask {
            package_id: format!("package_{}", i),
            package_path: format!("gno.land/p/demo/pkg{}", i),
            target_dir: PathBuf::from(format!("/tmp/pkg{}", i)),
            priority: i as u8,
            retry_config: RetryConfig::default(),
        };

        manager.queue_download(task).await.unwrap();
    }

    // Mock download function
    let download_count = Arc::new(AtomicUsize::new(0));
    let count_clone = Arc::clone(&download_count);

    let download_fn = move |_task: DownloadTask| {
        let count = Arc::clone(&count_clone);
        Box::pin(async move {
            // Simulate download
            sleep(Duration::from_millis(100)).await;
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }) as futures::future::BoxFuture<'static, Result<(), DownloadError>>
    };

    let summary = manager.process_queue(download_fn).await.unwrap();

    assert_eq!(summary.total_packages, 5);
    assert_eq!(summary.successful, 5);
    assert_eq!(summary.failed.len(), 0);
    assert_eq!(download_count.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn test_download_manager_with_failures() {
    let manager = DownloadManager::new(2);

    // Queue tasks
    for i in 0..4 {
        let task = DownloadTask {
            package_id: format!("package_{}", i),
            package_path: format!("gno.land/p/demo/pkg{}", i),
            target_dir: PathBuf::from(format!("/tmp/pkg{}", i)),
            priority: 0,
            retry_config: RetryConfig {
                max_attempts: 1,
                ..Default::default()
            },
        };

        manager.queue_download(task).await.unwrap();
    }

    // Mock download function that fails for even-numbered packages
    let download_fn = move |task: DownloadTask| {
        Box::pin(async move {
            if task.package_id.ends_with("0") || task.package_id.ends_with("2") {
                Err(DownloadError::Network("Simulated failure".to_string()))
            } else {
                Ok(())
            }
        }) as futures::future::BoxFuture<'static, Result<(), DownloadError>>
    };

    let summary = manager.process_queue(download_fn).await.unwrap();

    assert_eq!(summary.total_packages, 4);
    assert_eq!(summary.successful, 2);
    assert_eq!(summary.failed.len(), 2);
}

#[tokio::test]
async fn test_download_manager_priority() {
    let manager = DownloadManager::new(1); // Single concurrent download

    let execution_order = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    // Queue tasks with different priorities
    let tasks = vec![("low", 1), ("high", 10), ("medium", 5), ("critical", 20)];

    for (name, priority) in tasks {
        let task = DownloadTask {
            package_id: name.to_string(),
            package_path: format!("gno.land/p/demo/{}", name),
            target_dir: PathBuf::from(format!("/tmp/{}", name)),
            priority,
            retry_config: RetryConfig::default(),
        };

        manager.queue_download(task).await.unwrap();
    }

    let order_clone = Arc::clone(&execution_order);
    let download_fn = move |task: DownloadTask| {
        let order = Arc::clone(&order_clone);
        Box::pin(async move {
            order.lock().await.push(task.package_id);
            Ok(())
        }) as futures::future::BoxFuture<'static, Result<(), DownloadError>>
    };

    manager.process_queue(download_fn).await.unwrap();

    let final_order = execution_order.lock().await;
    // Should execute in priority order: critical, high, medium, low
    assert_eq!(final_order[0], "critical");
    assert_eq!(final_order[1], "high");
    assert_eq!(final_order[2], "medium");
    assert_eq!(final_order[3], "low");
}

#[tokio::test]
async fn test_download_manager_retry() {
    let manager = DownloadManager::new(1);

    let attempt_count = Arc::new(AtomicUsize::new(0));

    let task = DownloadTask {
        package_id: "retry_test".to_string(),
        package_path: "gno.land/p/demo/retry".to_string(),
        target_dir: PathBuf::from("/tmp/retry"),
        priority: 1,
        retry_config: RetryConfig {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(100),
            multiplier: 2.0,
        },
    };

    manager.queue_download(task).await.unwrap();

    let count_clone = Arc::clone(&attempt_count);
    let download_fn = move |_task: DownloadTask| {
        let count = Arc::clone(&count_clone);
        Box::pin(async move {
            let attempts = count.fetch_add(1, Ordering::SeqCst) + 1;
            if attempts < 3 {
                Err(DownloadError::Network("Retry me".to_string()))
            } else {
                Ok(())
            }
        }) as futures::future::BoxFuture<'static, Result<(), DownloadError>>
    };

    let summary = manager.process_queue(download_fn).await.unwrap();

    assert_eq!(summary.successful, 1);
    assert_eq!(summary.failed.len(), 0);
    assert_eq!(attempt_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_progress_tracker() {
    let manager = DownloadManager::new(2);
    let progress = manager.progress();

    // Get update receiver
    let update_rx = progress.get_update_receiver();

    // Queue a task
    let task = DownloadTask {
        package_id: "progress_test".to_string(),
        package_path: "gno.land/p/demo/progress".to_string(),
        target_dir: PathBuf::from("/tmp/progress"),
        priority: 1,
        retry_config: RetryConfig::default(),
    };

    manager.queue_download(task).await.unwrap();

    // Spawn a task to collect progress updates
    let updates = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let updates_clone = Arc::clone(&updates);
    let update_rx_clone = Arc::clone(&update_rx);

    let collector_task = tokio::spawn(async move {
        let mut rx = update_rx_clone.lock().await;
        while let Some(update) = rx.recv().await {
            updates_clone.lock().await.push(update);
        }
    });

    let download_fn = move |_task: DownloadTask| {
        Box::pin(async move {
            sleep(Duration::from_millis(10)).await;
            Ok(())
        }) as futures::future::BoxFuture<'static, Result<(), DownloadError>>
    };

    let _ = manager.process_queue(download_fn).await.unwrap();

    // Give time for updates to be processed
    sleep(Duration::from_millis(50)).await;

    // Cancel the collector task
    collector_task.abort();

    let collected_updates = updates.lock().await;

    // Should have at least Started and Completed updates
    assert!(
        collected_updates.len() >= 2,
        "Expected at least 2 updates, got {}",
        collected_updates.len()
    );

    // Check for Started update
    assert!(collected_updates
        .iter()
        .any(|u| matches!(u, ProgressUpdate::Started { .. })));

    // Check for Completed update
    assert!(collected_updates
        .iter()
        .any(|u| matches!(u, ProgressUpdate::Completed { .. })));
}

#[tokio::test]
async fn test_concurrent_downloads() {
    let manager = DownloadManager::new(4);

    let start_time = std::time::Instant::now();
    let concurrent_count = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));

    // Queue 8 tasks
    for i in 0..8 {
        let task = DownloadTask {
            package_id: format!("concurrent_{}", i),
            package_path: format!("gno.land/p/demo/concurrent{}", i),
            target_dir: PathBuf::from(format!("/tmp/concurrent{}", i)),
            priority: 0,
            retry_config: RetryConfig::default(),
        };

        manager.queue_download(task).await.unwrap();
    }

    let concurrent_clone = Arc::clone(&concurrent_count);
    let max_clone = Arc::clone(&max_concurrent);

    let download_fn = move |_task: DownloadTask| {
        let concurrent = Arc::clone(&concurrent_clone);
        let max = Arc::clone(&max_clone);

        Box::pin(async move {
            // Increment concurrent count
            let current = concurrent.fetch_add(1, Ordering::SeqCst) + 1;

            // Update max if needed
            let mut current_max = max.load(Ordering::SeqCst);
            while current > current_max {
                match max.compare_exchange(current_max, current, Ordering::SeqCst, Ordering::SeqCst)
                {
                    Ok(_) => break,
                    Err(actual) => current_max = actual,
                }
            }

            // Simulate work
            sleep(Duration::from_millis(100)).await;

            // Decrement concurrent count
            concurrent.fetch_sub(1, Ordering::SeqCst);

            Ok(())
        }) as futures::future::BoxFuture<'static, Result<(), DownloadError>>
    };

    let summary = manager.process_queue(download_fn).await.unwrap();

    let elapsed = start_time.elapsed();

    assert_eq!(summary.successful, 8);

    // With 4 concurrent downloads, 8 tasks should take about 200ms (2 batches)
    assert!(elapsed < Duration::from_millis(300));

    // Should have had at most 4 concurrent downloads
    assert!(max_concurrent.load(Ordering::SeqCst) <= 4);
}
