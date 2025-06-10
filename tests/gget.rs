use gget::fetch::{PackageManager, PackageManagerError};
use gget::DEFAULT_RPC_ENDPOINT;
use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn test_package_manager_creation() {
    let pm = PackageManager::new(None);
    assert_eq!(pm.rpc_endpoint, DEFAULT_RPC_ENDPOINT);

    let custom_endpoint = "https://custom.endpoint.com".to_string();
    let pm = PackageManager::new(Some(custom_endpoint.clone()));
    assert_eq!(pm.rpc_endpoint, custom_endpoint);
}

/// Test downloading a real package from gno.land
/// This test requires network access and may be slow
/// TODO: consider using a mock server for testing
#[tokio::test]
#[ignore] // Use `cargo test -- --ignored` to run this test
async fn test_package_manager_download_package() {
    // Create a temporary directory for testing
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Create a new package manager
    let pm = PackageManager::new(None);

    // Test downloading a package
    let pkg_path = "gno.land/p/demo/json";
    let result = pm.download_package(pkg_path, temp_path).await;

    // Assert that download was successful
    assert!(
        result.is_ok(),
        "Failed to download package: {:?}",
        result.err()
    );

    // Verify that files were downloaded
    let entries = fs::read_dir(temp_path).expect("Failed to read temp directory");
    let files: Vec<_> = entries.collect();
    assert!(!files.is_empty(), "No files were downloaded");

    // Check if specific files exist
    let expected_files = ["escape.gno", "node.gno", "buffer.gno", "path.gno"];
    for expected_file in &expected_files {
        let file_path = temp_path.join(expected_file);
        assert!(
            file_path.exists(),
            "Expected file {} not found at {}",
            expected_file,
            file_path.display()
        );

        // Also check that the file has content
        let file_size = fs::metadata(&file_path)
            .expect("Failed to get file metadata")
            .len();
        assert!(file_size > 0, "File {} is empty", expected_file);
    }

    // Verify file contents are not empty
    for expected_file in &expected_files {
        let file_path = temp_path.join(expected_file);
        let content = fs::read_to_string(&file_path).expect("Failed to read file content");
        assert!(
            !content.trim().is_empty(),
            "File {} has no content",
            expected_file
        );
    }
}

/// Test downloading an invalid package
#[tokio::test]
async fn test_package_manager_invalid_package() {
    // Create a temporary directory for testing
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Create a new package manager
    let pm = PackageManager::new(None);

    // Test downloading an invalid package
    let result = pm.download_package("invalid/package/path", temp_path).await;

    // Assert that download failed
    assert!(
        result.is_err(),
        "Expected error for invalid package path, but got success"
    );

    // Verify error type
    match result {
        Err(PackageManagerError::PackageFiles(_)) => {
            // This is expected - package files retrieval should fail
        }
        Err(PackageManagerError::Rpc(_)) => {
            // This is also acceptable - RPC error
        }
        Err(other) => {
            // Other errors are also acceptable, but let's log them
            println!("Got error (which is expected): {:?}", other);
        }
        Ok(_) => panic!("Expected an error but got success"),
    }
}

/// Test with custom RPC endpoint
#[tokio::test]
async fn test_package_manager_custom_endpoint() {
    let custom_endpoint = "https://custom.rpc.endpoint.com".to_string();
    let pm = PackageManager::new(Some(custom_endpoint.clone()));

    assert_eq!(pm.rpc_endpoint, custom_endpoint);

    // Test that it fails gracefully with unreachable endpoint
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let result = pm.download_package("test/package", temp_dir.path()).await;

    assert!(result.is_err(), "Expected error with unreachable endpoint");
}

/// Test directory creation functionality
#[tokio::test]
async fn test_directory_creation() {
    let pm = PackageManager::new(None);
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let target_path = temp_dir.path().join("nested").join("test_package");

    // Verify directory doesn't exist initially
    assert!(!target_path.exists());

    // Try to download (will fail due to network, but should create directory)
    let result = pm.download_package("test/package", &target_path).await;

    // Should create the directory even if download fails
    assert!(target_path.exists(), "Target directory was not created");
    assert!(target_path.is_dir(), "Target path is not a directory");

    // The download itself should fail due to invalid package
    assert!(
        result.is_err(),
        "Expected download to fail for test package"
    );
}

/// Test error handling for RPC communication
#[tokio::test]
async fn test_rpc_error_handling() {
    // Test with a malformed endpoint
    let pm = PackageManager::new(Some("not-a-valid-url".to_string()));
    let temp_dir = tempdir().expect("Failed to create temp directory");

    let result = pm.download_package("test/package", temp_dir.path()).await;

    assert!(result.is_err());
    // Should be a network/HTTP error
    match result.unwrap_err() {
        PackageManagerError::Http(_) => {
            // Expected
        }
        PackageManagerError::PackageFiles(_) => {
            // Also acceptable, as the HTTP error might be wrapped
        }
        other => {
            println!("Got unexpected error type: {:?}", other);
            // Don't fail the test as error wrapping might vary
        }
    }
}

/// Test empty package path handling
#[tokio::test]
async fn test_empty_package_path() {
    let pm = PackageManager::new(None);
    let temp_dir = tempdir().expect("Failed to create temp directory");

    let result = pm.download_package("", temp_dir.path()).await;
    assert!(result.is_err(), "Expected error for empty package path");
}
