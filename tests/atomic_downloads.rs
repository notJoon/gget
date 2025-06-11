use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::{io, time};
use tempfile::TempDir;
use tokio::fs;

use gget::cache::HybridCache;
use gget::fetch::PackageManagerError;

struct MockRpcServer {
    responses: Arc<Mutex<HashMap<String, String>>>,
    should_fail: Arc<Mutex<bool>>,
    call_count: Arc<Mutex<usize>>,
}

impl MockRpcServer {
    fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            should_fail: Arc::new(Mutex::new(false)),
            call_count: Arc::new(Mutex::new(0)),
        }
    }

    #[allow(dead_code)]
    fn set_rsps(&self, encoded_path: &str, response: &str) {
        let mut rsps = self.responses.lock().unwrap();
        rsps.insert(encoded_path.to_string(), response.to_string());
    }

    fn set_should_fail(&self, should_fail: bool) {
        *self.should_fail.lock().unwrap() = should_fail;
    }

    #[allow(dead_code)]
    fn get_call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }

    #[allow(dead_code)]
    fn reset_call_count(&self) {
        *self.call_count.lock().unwrap() = 0;
    }
}

#[allow(dead_code)]
struct MockPackageManager {
    cache: HybridCache,
    mock_server: MockRpcServer,
    rpc_endpoint: String,
}

impl MockPackageManager {
    fn new(cache_dir: PathBuf, mock_server: MockRpcServer) -> Self {
        let cache = HybridCache::new(cache_dir, time::Duration::from_secs(3600), 100);
        Self {
            cache,
            mock_server,
            rpc_endpoint: "http://mock.test".to_string(),
        }
    }

    // TODO: abstract this into a trait
    async fn download_package_atomic(
        &self,
        pkg_path: &str,
        target_dir: &std::path::Path,
    ) -> Result<(), PackageManagerError> {
        use std::time::{SystemTime, UNIX_EPOCH};

        // increment call count
        {
            let mut count = self.mock_server.call_count.lock().unwrap();
            *count += 1;
        }

        // check if should fail
        if *self.mock_server.should_fail.lock().unwrap() {
            return Err(PackageManagerError::Io(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "Mock server failed",
            )));
        }

        // create temp dir
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir_name = format!(
            "{}_tmp_{}",
            target_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("package"),
            timestamp
        );

        let temp_dir = if let Some(parent) = target_dir.parent() {
            parent.join(temp_dir_name)
        } else {
            PathBuf::from(temp_dir_name)
        };

        // Ensure cleanup happens even if download fails
        struct TempDirGuard(PathBuf);
        impl Drop for TempDirGuard {
            fn drop(&mut self) {
                if self.0.exists() {
                    let _ = std::fs::remove_dir_all(&self.0);
                }
            }
        }
        let _guard = TempDirGuard(temp_dir.clone());

        // Mock download to temporary directory
        std::fs::create_dir_all(&temp_dir).map_err(PackageManagerError::Io)?;

        // Create mock files based on package path
        match pkg_path {
            "gno.land/p/demo/avl" => {
                let avl_content = r#"package avl

type Node struct {
    key   string
    value any
    left  *Node
    right *Node
}

func NewTree() *Tree {
    return &Tree{}
}
"#;
                fs::write(temp_dir.join("node.gno"), avl_content)
                    .await
                    .map_err(PackageManagerError::Io)?;
                fs::write(
                    temp_dir.join("tree.gno"),
                    "package avl\n\ntype Tree struct{}\n",
                )
                .await
                .map_err(PackageManagerError::Io)?;
            }
            "gno.land/p/demo/ufmt" => {
                let ufmt_content = r#"package ufmt

func Sprintf(format string, args ...any) string {
    return ""
}

func Println(args ...any) {
    // implementation
}
"#;
                fs::write(temp_dir.join("ufmt.gno"), ufmt_content)
                    .await
                    .map_err(PackageManagerError::Io)?;
            }
            _ => {
                // Default mock package
                let content = format!(
                    "package {}\n\nfunc Hello() string {{\n    return \"Hello from {}\"\n}}",
                    pkg_path.split('/').last().unwrap_or("unknown"),
                    pkg_path
                );
                fs::write(temp_dir.join("main.gno"), content)
                    .await
                    .map_err(PackageManagerError::Io)?;
            }
        }

        // Validate the package (basic check)
        self.validate_package(&temp_dir).await?;

        // If target directory exists, remove it first
        if target_dir.exists() {
            std::fs::remove_dir_all(target_dir).map_err(PackageManagerError::Io)?;
        }

        // Create parent directory if needed
        if let Some(parent) = target_dir.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| PackageManagerError::DirectoryCreation(e.to_string()))?;
            }
        }

        // Atomically move from temp to final location
        std::fs::rename(&temp_dir, target_dir).map_err(PackageManagerError::Io)?;

        Ok(())
    }

    // TODO: abstract this into a trait
    async fn validate_package(
        &self,
        target_dir: &std::path::Path,
    ) -> Result<(), PackageManagerError> {
        let mut has_gno_files = false;

        if let Ok(entries) = std::fs::read_dir(target_dir) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "gno" {
                        has_gno_files = true;

                        // Basic validation - check if file is readable
                        let _content = std::fs::read_to_string(entry.path())
                            .map_err(PackageManagerError::Io)?;
                    }
                }
            }
        }

        if !has_gno_files {
            return Err(PackageManagerError::Rpc(
                "No valid .gno files found in package".to_string(),
            ));
        }

        Ok(())
    }
}

#[tokio::test]
async fn test_atomic_download_success() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().join("cache");
    let target_dir = temp_dir.path().join("avl");

    let mock_server = MockRpcServer::new();
    let package_manager = MockPackageManager::new(cache_dir, mock_server);

    // Test successful download
    let result = package_manager
        .download_package_atomic("gno.land/p/demo/avl", &target_dir)
        .await;

    assert!(result.is_ok(), "Download should succeed");
    assert!(target_dir.exists(), "Target directory should exist");
    assert!(
        target_dir.join("node.gno").exists(),
        "node.gno should exist"
    );
    assert!(
        target_dir.join("tree.gno").exists(),
        "tree.gno should exist"
    );

    // Verify content
    let content = fs::read_to_string(target_dir.join("node.gno"))
        .await
        .unwrap();
    assert!(content.contains("package avl"));
    assert!(content.contains("type Node struct"));
    assert!(content.contains("func NewTree() *Tree {"));
}

#[tokio::test]
async fn test_atomic_download_failure_cleanup() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().join("cache");
    let target_dir = temp_dir.path().join("failed_package");

    let mock_server = MockRpcServer::new();
    mock_server.set_should_fail(true); // Force failure
    let package_manager = MockPackageManager::new(cache_dir, mock_server);

    // Test failed download
    let result = package_manager
        .download_package_atomic("gno.land/p/demo/avl", &target_dir)
        .await;

    assert!(result.is_err(), "Download should fail");
    assert!(
        !target_dir.exists(),
        "Target directory should not exist after failure"
    );

    // Check that no temporary directories are left behind
    let parent_dir = target_dir.parent().unwrap();
    let entries: Vec<_> = std::fs::read_dir(parent_dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    let temp_dirs: Vec<_> = entries
        .iter()
        .filter(|entry| entry.file_name().to_string_lossy().contains("_tmp_"))
        .collect();

    assert_eq!(temp_dirs.len(), 0, "No temporary directories should remain");
}

#[tokio::test]
async fn test_atomic_download_preserves_existing_on_failure() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().join("cache");
    let target_dir = temp_dir.path().join("existing_package");

    // Create existing package
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(target_dir.join("existing.gno"), "package existing\n").unwrap();

    let _original_content = std::fs::read_to_string(target_dir.join("existing.gno")).unwrap();

    let mock_server = MockRpcServer::new();
    let package_manager = MockPackageManager::new(cache_dir, mock_server);

    // First download should succeed and replace existing
    let result = package_manager
        .download_package_atomic("gno.land/p/demo/avl", &target_dir)
        .await;
    assert!(result.is_ok());

    // Now set up for failure
    package_manager.mock_server.set_should_fail(true);

    // Attempted download should fail and preserve what was just downloaded
    let result = package_manager
        .download_package_atomic("gno.land/p/demo/ufmt", &target_dir)
        .await;

    assert!(result.is_err(), "Second download should fail");
    assert!(target_dir.exists(), "Target directory should still exist");

    // Should still contain the avl package files (from successful download)
    assert!(
        target_dir.join("node.gno").exists(),
        "Previous successful download should be preserved"
    );
    assert!(
        !target_dir.join("ufmt.gno").exists(),
        "Failed download files should not exist"
    );
}

#[tokio::test]
async fn test_atomic_download_overwrites_existing_on_success() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().join("cache");
    let target_dir = temp_dir.path().join("overwrite_test");

    // Create existing package
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(target_dir.join("old.gno"), "package old\n").unwrap();

    let mock_server = MockRpcServer::new();
    let package_manager = MockPackageManager::new(cache_dir, mock_server);

    // Download new package should overwrite existing
    let result = package_manager
        .download_package_atomic("gno.land/p/demo/avl", &target_dir)
        .await;

    assert!(result.is_ok(), "Download should succeed");
    assert!(target_dir.exists(), "Target directory should exist");
    assert!(
        !target_dir.join("old.gno").exists(),
        "Old file should be removed"
    );
    assert!(
        target_dir.join("node.gno").exists(),
        "New file should exist"
    );
    assert!(
        target_dir.join("tree.gno").exists(),
        "New file should exist"
    );
}

#[tokio::test]
async fn test_concurrent_atomic_downloads() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().join("cache");

    let mock_server = MockRpcServer::new();
    let package_manager = Arc::new(MockPackageManager::new(cache_dir, mock_server));

    // Test concurrent downloads to different directories
    let handles: Vec<_> = (0..3)
        .map(|i| {
            let pm = Arc::clone(&package_manager);
            let target = temp_dir.path().join(format!("concurrent_{}", i));
            tokio::spawn(async move {
                pm.download_package_atomic("gno.land/p/demo/avl", &target)
                    .await
            })
        })
        .collect();

    // Wait for all downloads to complete
    let results: Vec<_> = futures::future::join_all(handles).await;

    // All downloads should succeed
    for (i, result) in results.into_iter().enumerate() {
        let download_result = result.unwrap(); // unwrap the JoinResult
        assert!(
            download_result.is_ok(),
            "Concurrent download {} should succeed",
            i
        );

        let target = temp_dir.path().join(format!("concurrent_{}", i));
        assert!(target.exists(), "Target directory {} should exist", i);
        assert!(
            target.join("node.gno").exists(),
            "node.gno should exist in dir {}",
            i
        );
    }
}

#[tokio::test]
async fn test_atomic_download_validation_failure() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().join("cache");
    let target_dir = temp_dir.path().join("invalid_package");

    let mock_server = MockRpcServer::new();
    let package_manager = MockPackageManager::new(cache_dir, mock_server);

    // Create a custom implementation that creates invalid package
    struct InvalidMockPackageManager {
        inner: MockPackageManager,
    }

    impl InvalidMockPackageManager {
        async fn download_package_atomic_invalid(
            &self,
            _pkg_path: &str,
            target_dir: &std::path::Path,
        ) -> Result<(), PackageManagerError> {
            use std::time::{SystemTime, UNIX_EPOCH};

            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let temp_dir_name = format!(
                "{}_tmp_{}",
                target_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("package"),
                timestamp
            );

            let temp_dir = if let Some(parent) = target_dir.parent() {
                parent.join(temp_dir_name)
            } else {
                PathBuf::from(temp_dir_name)
            };

            struct TempDirGuard(PathBuf);
            impl Drop for TempDirGuard {
                fn drop(&mut self) {
                    if self.0.exists() {
                        let _ = std::fs::remove_dir_all(&self.0);
                    }
                }
            }
            let _guard = TempDirGuard(temp_dir.clone());

            // Create temp dir but no .gno files (will fail validation)
            std::fs::create_dir_all(&temp_dir).map_err(PackageManagerError::Io)?;
            std::fs::write(temp_dir.join("README.md"), "Not a gno file")
                .map_err(PackageManagerError::Io)?;

            // This should fail validation
            self.inner.validate_package(&temp_dir).await?;

            Ok(())
        }
    }

    let invalid_manager = InvalidMockPackageManager {
        inner: package_manager,
    };

    // Test download with validation failure
    let result = invalid_manager
        .download_package_atomic_invalid("gno.land/p/demo/invalid", &target_dir)
        .await;

    assert!(result.is_err(), "Download should fail validation");
    assert!(
        !target_dir.exists(),
        "Target directory should not exist after validation failure"
    );

    // Check that no temporary directories are left behind
    let parent_dir = target_dir.parent().unwrap();
    let entries: Vec<_> = std::fs::read_dir(parent_dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    let temp_dirs: Vec<_> = entries
        .iter()
        .filter(|entry| entry.file_name().to_string_lossy().contains("_tmp_"))
        .collect();

    assert_eq!(
        temp_dirs.len(),
        0,
        "No temporary directories should remain after validation failure"
    );
}
