use base64::{engine::general_purpose, Engine as _};
use reqwest::{Client, Error as ReqwestError};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

use crate::cache::{CacheError, HybridCache};
use crate::dependency::{DependencyError, DependencyResolver, PackageDependency};
use crate::parallel::{
    DownloadError, DownloadManager, DownloadSummary, DownloadTask, ParallelDownloadOptions,
};
use crate::query::{RpcParams, RpcRequest, RpcResponse};
use crate::DEFAULT_RPC_ENDPOINT;

const MAX_ENTRIES: u64 = 1_000;
const TTL: u64 = 24 * 3600;

#[derive(Error, Debug)]
pub enum PackageManagerError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] ReqwestError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Base64 decoding error: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("RPC error: {0}")]
    Rpc(String),

    #[error("Failed to create target directory: {0}")]
    DirectoryCreation(String),

    #[error("Failed to get package files: {0}")]
    PackageFiles(String),

    #[error("Failed to get file content for {file}: {error}")]
    FileContent { file: String, error: String },

    #[error("Cache error: {0}")]
    Cache(#[from] CacheError),

    #[error("Dependency error: {0}")]
    Dependency(#[from] DependencyError),
}

#[derive(Clone)]
pub struct PackageManager {
    rpc_endpoint: String,
    http_client: Client,
    cache: Arc<HybridCache>,
}

impl PackageManager {
    /// Creates a new PackageManager instance
    pub fn new(rpc_endpoint: Option<String>, cache_dir: PathBuf) -> Self {
        let endpoint = rpc_endpoint.unwrap_or_else(|| DEFAULT_RPC_ENDPOINT.to_string());
        let http_client = Client::new();
        let cache = HybridCache::new(cache_dir, Duration::from_secs(TTL), MAX_ENTRIES);

        Self {
            rpc_endpoint: endpoint,
            http_client,
            cache: Arc::new(cache),
        }
    }

    /// Returns the RPC endpoint
    pub fn rpc_endpoint(&self) -> &str {
        &self.rpc_endpoint
    }

    /// Downloads a package and its files to the target directory
    pub async fn download_package(
        &self,
        pkg_path: &str,
        target_dir: &Path,
    ) -> Result<(), PackageManagerError> {
        // Create target directory if it doesn't exist
        if !target_dir.exists() {
            fs::create_dir_all(target_dir)
                .map_err(|e| PackageManagerError::DirectoryCreation(e.to_string()))?;
        }

        let files_key = format!("files:{}", pkg_path);
        let files: Vec<String> = if let Some(raw) = self.cache.get(&files_key).await? {
            serde_json::from_str(&raw)?
        } else {
            let list = self
                .get_package_files(pkg_path)
                .await
                .map_err(|e| PackageManagerError::PackageFiles(e.to_string()))?;
            let serialized = serde_json::to_string(&list)?;
            self.cache.set(&files_key, &serialized).await?;
            list
        };

        // for each file, fetch content via cache or RPC
        for file in files {
            let trimmed = file.trim();
            if trimmed.is_empty() {
                continue;
            }
            let file_path = format!("{}/{}", pkg_path, trimmed);
            let content_key = format!("file:{}", file_path);
            let content = if let Some(raw) = self.cache.get(&content_key).await? {
                raw
            } else {
                let cnt = self.get_file_content(&file_path).await.map_err(|e| {
                    PackageManagerError::FileContent {
                        file: file.clone(),
                        error: e.to_string(),
                    }
                })?;
                self.cache.set(&content_key, &cnt).await?;
                cnt
            };

            // write to disk
            let target = target_dir.join(&file);
            if let Some(p) = target.parent() {
                fs::create_dir_all(p)?;
            }
            fs::write(&target, &content)?;
            println!("Downloaded: {}", target.display());
        }

        Ok(())
    }

    /// Downloads a package atomically to prevent partial downloads
    pub async fn download_package_atomic(
        &self,
        pkg_path: &str,
        target_dir: &Path,
    ) -> Result<(), PackageManagerError> {
        use std::time::{SystemTime, UNIX_EPOCH};

        // create a unique temp dir name
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir_name = format!(
            "{}_tmp_{}",
            target_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("package"),
            timestamp,
        );

        let temp_dir = if let Some(parent) = target_dir.parent() {
            parent.join(temp_dir_name)
        } else {
            PathBuf::from(temp_dir_name)
        };

        // ensure cleanup happens even if download fails
        // automatically remove temp dir on drop with RAII pattern
        struct TempDirGuard(PathBuf);
        impl Drop for TempDirGuard {
            fn drop(&mut self) {
                if self.0.exists() {
                    let _ = std::fs::remove_dir_all(&self.0);
                }
            }
        }

        let _guard = TempDirGuard(temp_dir.clone());

        // download to temp dir first
        self.download_package(pkg_path, &temp_dir).await?;

        // if target dir exists, remove it
        if target_dir.exists() {
            std::fs::remove_dir_all(target_dir).map_err(PackageManagerError::Io)?;
        }

        // create parent dir if it doesn't exist
        if let Some(p) = target_dir.parent() {
            if !p.exists() {
                std::fs::create_dir_all(p)
                    .map_err(|e| PackageManagerError::DirectoryCreation(e.to_string()))?;
            }
        }

        // atomically move from temp to final destination
        std::fs::rename(&temp_dir, target_dir).map_err(PackageManagerError::Io)?;

        Ok(())
    }

    #[allow(dead_code)]
    async fn resolve_all_dependencies(
        &self,
        root_pkg: &str,
    ) -> Result<HashMap<String, String>, PackageManagerError> {
        let mut all_deps = HashMap::new();
        let mut to_analyze = VecDeque::new();
        let mut analyzed = HashSet::new();

        to_analyze.push_back(root_pkg.to_string());

        while let Some(pkg_path) = to_analyze.pop_front() {
            if analyzed.contains(&pkg_path) {
                continue;
            }

            let package_dep = self.analyze_package_dependencies(&pkg_path).await?;

            // add new deps to analysis queue
            for import in &package_dep.imports {
                if !analyzed.contains(import) && !to_analyze.contains(import) {
                    to_analyze.push_back(import.clone());
                }
            }

            // add to result map
            all_deps.insert(pkg_path.clone(), package_dep.name);
            analyzed.insert(pkg_path);
        }

        Ok(all_deps)
    }

    #[allow(dead_code)]
    async fn analyze_package_dependencies(
        &self,
        pkg_path: &str,
    ) -> Result<PackageDependency, PackageManagerError> {
        let files = self.get_package_files(pkg_path).await?;
        let mut all_imports = HashSet::new();

        let mut resolver = DependencyResolver::new()?;

        for file in files {
            let trimmed = file.trim();
            if trimmed.is_empty() || !trimmed.ends_with(".gno") {
                continue;
            }

            let file_path = format!("{}/{}", pkg_path, trimmed);
            let content = self.get_file_content(&file_path).await?;

            // reuse the same resolver instance for all files in the same package
            let (_, imports) = resolver.extract_dependencies(&content)?;
            all_imports.extend(imports);
        }

        Ok(PackageDependency {
            name: pkg_path.to_string(),
            imports: all_imports,
            instability: 0.0,
        })
    }

    pub async fn validate_package(&self, target_dir: &Path) -> Result<(), PackageManagerError> {
        // when users deploy packages to the chain, the `gnokey` only recognizes and deploys
        // `gno.mod` and `*.gno` files. Therefore, this check is actually meaningless.
        let mut resolver = DependencyResolver::new()?;

        // Use the new directory-based method to validate all .gno files recursively
        let packages = resolver.extract_dependencies_from_directory(target_dir)?;

        if packages.is_empty() {
            return Err(PackageManagerError::PackageFiles(
                "No .gno files found".to_string(),
            ));
        }

        // All files were successfully parsed if we got here
        Ok(())
    }

    /// Retrieves the list of files in a package
    async fn get_package_files(&self, pkg_path: &str) -> Result<Vec<String>, PackageManagerError> {
        let encoded_path = general_purpose::STANDARD.encode(pkg_path.as_bytes());
        let data = self.query_rpc(&encoded_path).await?;

        // Decode the response data
        let decoded_data = general_purpose::STANDARD.decode(&data)?;
        let files_list = String::from_utf8_lossy(&decoded_data);

        // Split the file list and filter out empty strings
        let files: Vec<String> = files_list
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(files)
    }

    /// Retrieves the content of a specific file
    async fn get_file_content(&self, file_path: &str) -> Result<String, PackageManagerError> {
        let encoded_path = general_purpose::STANDARD.encode(file_path.as_bytes());
        let data = self.query_rpc(&encoded_path).await?;

        // Decode the response data
        let decoded_data = general_purpose::STANDARD.decode(&data)?;
        let content = String::from_utf8_lossy(&decoded_data).to_string();

        Ok(content)
    }

    /// Sends a query to the RPC endpoint (core function)
    async fn query_rpc(&self, data: &str) -> Result<String, PackageManagerError> {
        let request = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "abci_query".to_string(),
            params: RpcParams {
                path: "vm/qfile".to_string(),
                data: data.to_string(),
            },
        };

        let response = self
            .http_client
            .post(&self.rpc_endpoint)
            .json(&request)
            .send()
            .await?;

        let rpc_response: RpcResponse = response.json().await?;

        if let Some(error) = rpc_response.result.response.response_base.error {
            return Err(PackageManagerError::Rpc(format!("RPC error: {}", error)));
        }

        Ok(rpc_response.result.response.response_base.data)
    }

    /// Download multiple packages concurrently
    /// TODO: should be default method.
    pub async fn download_packages_parallel(
        &self,
        packages: Vec<&str>,
        target_dir: &Path,
        options: ParallelDownloadOptions,
    ) -> Result<DownloadSummary, PackageManagerError> {
        let download_manager = DownloadManager::new(options.max_concurrent);

        // Queue all packages
        for (idx, package) in packages.iter().enumerate() {
            let task = DownloadTask {
                package_id: package.to_string(),
                package_path: package.to_string(),
                target_dir: target_dir.join(package),
                priority: (packages.len() - idx) as u8, // Earlier packages have higher priority
                retry_config: options.retry_config.clone(),
            };
            download_manager
                .queue_download(task)
                .await
                .map_err(|e| PackageManagerError::Rpc(e.to_string()))?;
        }

        // Create a closure that captures self for downloading
        let self_clone = self.clone();
        let download_fn = move |task: DownloadTask| {
            let pm = self_clone.clone();
            Box::pin(async move {
                pm.download_package(&task.package_path, &task.target_dir)
                    .await
                    .map_err(|e| DownloadError::PackageManager(e))
            }) as futures::future::BoxFuture<'static, Result<(), DownloadError>>
        };

        // Process queue with progress tracking
        let summary = download_manager
            .process_queue(download_fn)
            .await
            .map_err(|e| PackageManagerError::Rpc(e.to_string()))?;

        // Print summary if progress is enabled
        if options.show_progress {
            println!("\n{}", summary);
        }

        Ok(summary)
    }

    /// Download package with its dependencies in parallel
    pub async fn download_with_deps_parallel(
        &self,
        package: &str,
        target_dir: &Path,
        options: ParallelDownloadOptions,
    ) -> Result<DownloadSummary, PackageManagerError> {
        println!("Analyzing dependencies for {}...", package);

        // First, analyze all dependencies
        let all_deps = self.resolve_all_dependencies(package).await?;

        // Convert to package list
        let mut packages: Vec<&str> = all_deps.keys().map(|s| s.as_str()).collect();

        // Sort packages for consistent ordering
        packages.sort();

        println!("Found {} packages to download", packages.len());

        // Download all packages in parallel
        self.download_packages_parallel(packages, target_dir, options)
            .await
    }
}
