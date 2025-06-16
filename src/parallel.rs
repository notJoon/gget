use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, Mutex, Semaphore};

use crate::fetch::PackageManagerError;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Timeout after {0:?}")]
    Timeout(Duration),

    #[error("Checksum mismatch")]
    ChecksumMismatch,

    #[error("Download cancelled")]
    Cancelled,

    #[error("Max retries exceeded")]
    MaxRetriesExceeded,

    #[error("Package manager error: {0}")]
    PackageManager(#[from] PackageManagerError),
}

#[derive(Debug, Clone)]
pub struct DownloadTask {
    /// Package identifier
    pub package_id: String,
    /// Package path (e.g., "gno.land/p/demo/avl")
    pub package_path: String,
    /// Target directory for download
    pub target_dir: PathBuf,
    /// Priority (higher = more important)
    pub priority: u8,
    /// Retry configuration
    pub retry_config: RetryConfig,
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum retry attempts
    pub max_attempts: u32,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Backoff multiplier
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            multiplier: 2.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PackageProgress {
    pub package_id: String,
    pub state: DownloadState,
    pub started_at: Instant,
    pub eta: Option<Duration>,
}

#[derive(Debug, Clone)]
pub enum DownloadState {
    Queued,
    Downloading { percent: f32 },
    Completed,
    Failed { error: String },
    Cancelled,
}

#[derive(Debug)]
pub struct DownloadSummary {
    pub total_packages: usize,
    pub successful: usize,
    pub failed: Vec<FailedDownload>,
    pub duration: Duration,
}

#[derive(Debug)]
pub struct FailedDownload {
    pub package: String,
    pub error: DownloadError,
    pub retry_count: u32,
}

#[derive(Debug, Clone)]
pub struct ParallelDownloadOptions {
    /// Maximum concurrent downloads
    pub max_concurrent: usize,
    /// Enable progress display
    pub show_progress: bool,
    /// Retry configuration
    pub retry_config: RetryConfig,
    /// Timeout per download
    pub timeout: Duration,
}

impl Default for ParallelDownloadOptions {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            show_progress: true,
            retry_config: RetryConfig::default(),
            timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

pub struct ProgressTracker {
    /// Progress for each package
    package_progress: Arc<Mutex<HashMap<String, PackageProgress>>>,
    /// Update channel for progress events
    update_tx: mpsc::Sender<ProgressUpdate>,
    update_rx: Arc<Mutex<mpsc::Receiver<ProgressUpdate>>>,
}

#[derive(Debug)]
pub enum ProgressUpdate {
    Started { package_id: String },
    Progress { package_id: String, percent: f32 },
    Completed { package_id: String },
    Failed { package_id: String, error: String },
}

impl ProgressTracker {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            package_progress: Arc::new(Mutex::new(HashMap::new())),
            update_tx: tx,
            update_rx: Arc::new(Mutex::new(rx)),
        }
    }

    pub async fn update(&self, update: ProgressUpdate) {
        let _ = self.update_tx.send(update).await;
    }

    pub async fn get_progress(&self) -> HashMap<String, PackageProgress> {
        self.package_progress.lock().await.clone()
    }

    pub fn get_update_receiver(&self) -> Arc<Mutex<mpsc::Receiver<ProgressUpdate>>> {
        Arc::clone(&self.update_rx)
    }
}

pub struct DownloadManager {
    /// Semaphore for concurrency control
    semaphore: Arc<Semaphore>,
    /// Progress tracking
    progress: Arc<ProgressTracker>,
    /// Download queue
    queue: Arc<Mutex<VecDeque<DownloadTask>>>,
}

impl DownloadManager {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            progress: Arc::new(ProgressTracker::new()),
            queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Queue a package for download
    pub async fn queue_download(&self, task: DownloadTask) -> Result<(), DownloadError> {
        let mut queue = self.queue.lock().await;

        // Insert based on priority (higher priority first)
        let position = queue
            .iter()
            .position(|t| t.priority < task.priority)
            .unwrap_or(queue.len());

        queue.insert(position, task);
        Ok(())
    }

    /// Process all queued downloads
    pub async fn process_queue<F>(&self, download_fn: F) -> Result<DownloadSummary, DownloadError>
    where
        F: Fn(DownloadTask) -> futures::future::BoxFuture<'static, Result<(), DownloadError>>
            + Send
            + Sync
            + 'static,
    {
        let start_time = Instant::now();
        let download_fn = Arc::new(download_fn);
        let mut handles = Vec::new();
        let mut total_packages = 0;

        // Process queue
        loop {
            let task = {
                let mut queue = self.queue.lock().await;
                queue.pop_front()
            };

            let Some(task) = task else {
                break;
            };

            total_packages += 1;
            let package_id = task.package_id.clone();
            let package_id_for_handle = package_id.clone();

            // Update progress
            self.progress
                .update(ProgressUpdate::Started {
                    package_id: package_id.clone(),
                })
                .await;

            // Acquire semaphore permit
            let permit = Arc::clone(&self.semaphore);
            let progress = Arc::clone(&self.progress);
            let download_fn = Arc::clone(&download_fn);

            let handle = tokio::spawn(async move {
                let _permit = permit.acquire().await.unwrap();
                let result = Self::download_with_retry(task, download_fn.as_ref(), &progress).await;

                match &result {
                    Ok(_) => {
                        progress
                            .update(ProgressUpdate::Completed {
                                package_id: package_id.clone(),
                            })
                            .await;
                    }
                    Err(e) => {
                        progress
                            .update(ProgressUpdate::Failed {
                                package_id: package_id.clone(),
                                error: e.to_string(),
                            })
                            .await;
                    }
                }

                result
            });

            handles.push((package_id_for_handle, handle));
        }

        // Wait for all downloads to complete
        let mut successful = 0;
        let mut failed = Vec::new();

        for (package_id, handle) in handles {
            match handle.await {
                Ok(Ok(_)) => successful += 1,
                Ok(Err(e)) => {
                    failed.push(FailedDownload {
                        package: package_id,
                        error: e,
                        retry_count: 0, // Will be updated by retry logic
                    });
                }
                Err(e) => {
                    failed.push(FailedDownload {
                        package: package_id,
                        error: DownloadError::Network(format!("Task panic: {}", e)),
                        retry_count: 0,
                    });
                }
            }
        }

        let duration = start_time.elapsed();

        Ok(DownloadSummary {
            total_packages,
            successful,
            failed,
            duration,
        })
    }

    /// Download with retry logic
    async fn download_with_retry<F>(
        task: DownloadTask,
        download_fn: &F,
        _progress: &ProgressTracker,
    ) -> Result<(), DownloadError>
    where
        F: Fn(DownloadTask) -> futures::future::BoxFuture<'static, Result<(), DownloadError>>,
    {
        let mut attempts = 0;
        let mut backoff = task.retry_config.initial_backoff;

        loop {
            attempts += 1;

            match download_fn(task.clone()).await {
                Ok(_) => return Ok(()),
                Err(_e) if attempts >= task.retry_config.max_attempts => {
                    return Err(DownloadError::MaxRetriesExceeded);
                }
                Err(e) => {
                    // Log retry attempt
                    eprintln!(
                        "Download failed for {}: {}. Retrying in {:?} (attempt {}/{})",
                        task.package_id, e, backoff, attempts, task.retry_config.max_attempts
                    );

                    // Wait before retry
                    tokio::time::sleep(backoff).await;

                    // Update backoff
                    backoff = std::cmp::min(
                        backoff.mul_f64(task.retry_config.multiplier),
                        task.retry_config.max_backoff,
                    );
                }
            }
        }
    }

    /// Get progress tracker
    pub fn progress(&self) -> &ProgressTracker {
        &self.progress
    }
}

impl std::fmt::Display for DownloadSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Downloaded {} packages in {:?} ({} successful, {} failed)",
            self.total_packages,
            self.duration,
            self.successful,
            self.failed.len()
        )
    }
}
