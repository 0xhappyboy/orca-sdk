use std::{sync::Arc, time::Duration};

use tokio::sync::mpsc;

use crate::{OrcaClient, types::OrcaResult};

impl OrcaClient {
    /// Monitors price changes for a given pool with production-ready error handling and configurable thresholds.
    ///
    /// # Params
    ///
    /// pool_address - The address of the liquidity pool to monitor
    /// min_change_percent - Minimum percentage change required to trigger callback
    /// callback - Function called when significant price change is detected
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::sync::Arc;
    /// use orca_sdk::OrcaClient;
    ///
    /// let client = Arc::new(OrcaClient::new().await?);
    /// let pool_address = "POOL_ADDRESS_HERE";
    ///
    /// let monitor_handle = client.monitor_price_changes_production(
    ///     pool_address,
    ///     1.0, // 1% minimum change
    ///     |update| {
    ///         println!("Price changed: {}%", update.change_percent);
    ///         println!("Old price: {}, New price: {}", update.old_price, update.new_price);
    ///     },
    /// ).await?;
    /// ```
    pub async fn monitor_price_changes_production<F>(
        self: Arc<Self>,
        pool_address: &str,
        min_change_percent: f64,
        callback: F,
    ) -> OrcaResult<PriceMonitorHandle>
    where
        F: Fn(PriceUpdate) + Send + Sync + 'static,
    {
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        let client = self;
        let pool_address = pool_address.to_string();
        let handle = tokio::spawn(async move {
            let mut last_price: Option<f64> = None;
            let mut consecutive_errors = 0;
            const MAX_CONSECUTIVE_ERRORS: u32 = 5;
            const POLL_INTERVAL: Duration = Duration::from_secs(10);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(POLL_INTERVAL) => {}
                    _ = shutdown_rx.recv() => {
                        log::info!("Price monitor for {} shutting down", pool_address);
                        break;
                    }
                }
                let client_clone = client.clone();
                // 使用克隆的客户端获取价格
                match Self::get_current_price_impl(&client_clone, &pool_address).await {
                    Ok(current_price) => {
                        consecutive_errors = 0;
                        if let Some(prev_price) = last_price {
                            let prev_price: f64 = prev_price;
                            let current_price: f64 = current_price;
                            if prev_price > 0.0 {
                                let change_percent =
                                    ((current_price - prev_price) / prev_price).abs() * 100.0;
                                if change_percent >= min_change_percent {
                                    callback(PriceUpdate {
                                        pool_address: pool_address.clone(),
                                        old_price: prev_price,
                                        new_price: current_price,
                                        change_percent,
                                        timestamp: chrono::Utc::now(),
                                    });
                                }
                            }
                        }
                        last_price = Some(current_price);
                    }
                    Err(_e) => {
                        consecutive_errors += 1;
                        if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                            log::error!(
                                "Too many consecutive errors, shutting down monitor for {}",
                                pool_address
                            );
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                }
            }
        });

        Ok(PriceMonitorHandle {
            shutdown_tx,
            task_handle: handle,
        })
    }

    /// Internal implementation for fetching current price from on-chain data
    async fn get_current_price_impl(client: &OrcaClient, pool_address: &str) -> OrcaResult<f64> {
        // 使用已有的池子状态获取价格
        let pool_info = client.get_pool_state_onchain(pool_address).await?;
        // 使用第一个代币作为基准计算价格
        let base_mint = &pool_info.token_mint_a;
        client
            .derive_price_from_pool_state(&pool_info, base_mint)
            .await
    }
}

/// Handle for controlling a price monitoring task
///
/// Use this handle to gracefully shutdown the monitoring task
/// when it's no longer needed.
///
/// # Examples
///
/// ```rust
/// let client = std::sync::Arc::new(orca_sdk::OrcaClient::new().await?);
/// let monitor_handle = client.monitor_price_changes_production(
///     "POOL_ADDRESS",
///     1.0,
///     |_| {},
/// ).await?;
///
/// // Shutdown the monitor when done
/// monitor_handle.shutdown().await;
/// ```
#[derive(Debug)]
pub struct PriceMonitorHandle {
    shutdown_tx: mpsc::Sender<()>,
    task_handle: tokio::task::JoinHandle<()>,
}

impl PriceMonitorHandle {
    /// Gracefully shuts down the price monitoring task
    ///
    /// Sends a shutdown signal to the monitoring task and waits
    /// for it to complete cleanup.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(()).await;
        let _ = self.task_handle.await;
    }
}
/// Represents a significant price change event
///
/// Contains all relevant information about a price change
/// that exceeded the configured threshold.
#[derive(Debug, Clone)]
pub struct PriceUpdate {
    /// Address of the pool where the price change occurred
    pub pool_address: String,
    /// Price before the change
    pub old_price: f64,
    /// Current price after the change
    pub new_price: f64,
    /// Percentage change between old and new price
    pub change_percent: f64,
    /// Timestamp when the change was detected
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
