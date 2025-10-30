use solana_commitment_config::CommitmentConfig;
use solana_transaction_status::UiTransactionEncoding;

use super::*;
use crate::{pool::PoolInfo, types::OrcaResult};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PriceData {
    pub timestamp: u64,
    pub price: f64,
    pub liquidity: u128,
}

#[derive(Debug, Clone)]
pub struct PriceAlert {
    pub token_pair: String,
    pub target_price: f64,
    pub condition: PriceCondition,
}

#[derive(Debug, Clone)]
pub enum PriceCondition {
    Above,
    Below,
}

#[derive(Debug, Clone)]
pub struct PriceMonitor {
    alerts: HashMap<String, Vec<PriceAlert>>,
}

impl OrcaClient {
    /// Monitors the health of a liquidity pool by analyzing key metrics.
    ///
    /// # Params
    /// pool_address - The address of the pool to monitor
    ///
    /// # Returns
    /// Returns a `PoolHealth` struct containing liquidity, volume, fee growth, and health score
    ///
    /// # Example
    /// ```no_run
    /// use orca_client::OrcaClient;
    ///
    /// tokio_test::block_on(async {
    /// let client = OrcaClient::new();
    /// let pool_health = client.monitor_pool_health("POOL_ADDRESS_HERE").await.unwrap();
    /// println!("Pool health score: {}", pool_health.health_score);
    /// });
    /// ```
    pub async fn monitor_pool_health(&self, pool_address: &str) -> OrcaResult<PoolHealth> {
        let pool_info = self.get_pool_state_onchain(pool_address).await?;
        let liquidity = pool_info.liquidity;
        let volume_24h = self.estimate_24h_volume(&pool_info).await?;
        let fee_growth = pool_info.fee_growth_global_a + pool_info.fee_growth_global_b;
        Ok(PoolHealth {
            liquidity,
            volume_24h,
            fee_growth,
            health_score: self.calculate_health_score(liquidity, volume_24h, fee_growth),
        })
    }

    /// Estimates 24-hour trading volume using multiple reliable methods.
    ///
    /// Combines fee-based estimation and transaction count analysis for robust volume calculation.
    async fn estimate_24h_volume(&self, pool: &PoolInfo) -> OrcaResult<u64> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let pool_pubkey = Pubkey::from_str(&pool.address)
            .map_err(|e| OrcaError::Error(format!("Invalid pool address: {}", e)))?;
        let volume_from_fees = self.estimate_volume_from_fee_growth(pool).await?;
        let volume_from_tx_count = self.estimate_volume_from_tx_count(&pool_pubkey).await?;
        Ok(volume_from_fees.max(volume_from_tx_count))
    }

    /// Estimates trading volume based on fee growth data.
    ///
    /// This is the most stable and reliable method for volume estimation.
    async fn estimate_volume_from_fee_growth(&self, pool: &PoolInfo) -> OrcaResult<u64> {
        let total_fee_growth = pool.fee_growth_global_a + pool.fee_growth_global_b;
        const FEE_RATE: f64 = 0.003;
        let estimated_volume = (total_fee_growth as f64 / FEE_RATE) as u64;
        Ok(estimated_volume.min((u64::MAX as u128).try_into().unwrap()) as u64)
    }

    /// Estimates trading volume based on transaction count analysis.
    ///
    /// Uses recent transaction samples to extrapolate daily volume.
    async fn estimate_volume_from_tx_count(&self, pool_pubkey: &Pubkey) -> OrcaResult<u64> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let signatures = client
            .get_signatures_for_address(pool_pubkey)
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get signatures: {}", e)))?;
        let mut total_sample_volume = 0u64;
        let mut sample_count = 0;
        for sig_info in signatures.iter().take(20) {
            if let Some(volume) = self.estimate_single_tx_volume(&sig_info.signature).await? {
                total_sample_volume += volume;
                sample_count += 1;
            }
        }
        if sample_count == 0 {
            return Ok(0);
        }
        let avg_tx_volume = total_sample_volume / sample_count;
        let estimated_daily_tx_count = signatures.len().min(1000); // 保守估计
        Ok(avg_tx_volume * estimated_daily_tx_count as u64)
    }

    /// Estimates volume for a single transaction using multiple approaches.
    ///
    /// # Params
    /// signature - The transaction signature to analyze
    ///
    /// # Returns
    /// Returns estimated volume if successful, None if transaction cannot be analyzed
    ///
    /// # Example
    /// ```no_run
    /// use orca_client::OrcaClient;
    ///
    /// tokio_test::block_on(async {
    /// let client = OrcaClient::new();
    /// let volume = client.estimate_single_tx_volume("SIGNATURE_HERE").await.unwrap();
    /// println!("Estimated transaction volume: {:?}", volume);
    /// });
    /// ```
    async fn estimate_single_tx_volume(&self, signature: &str) -> OrcaResult<Option<u64>> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let signature = Signature::from_str(signature)
            .map_err(|e| OrcaError::Error(format!("Invalid signature: {}", e)))?;
        let transaction = client
            .get_transaction_with_config(
                &signature,
                solana_client::rpc_config::RpcTransactionConfig {
                    encoding: Some(UiTransactionEncoding::JsonParsed),
                    commitment: Some(CommitmentConfig::confirmed()),
                    max_supported_transaction_version: Some(0),
                },
            )
            .await;
        match transaction {
            Ok(tx_response) => {
                if let Some(meta) = &tx_response.transaction.meta {
                    let fee = meta.fee;
                    let estimated_volume = (fee as f64 / 0.003) as u64;
                    return Ok(Some(estimated_volume));
                }
                if let Some(logs) = &tx_response
                    .transaction
                    .meta
                    .and_then(|m| Some(m.log_messages))
                {
                    for log in logs.clone().unwrap() {
                        if log.contains("swap") || log.contains("amount") || log.contains("Swap") {
                            if let Some(amount) = Self::extract_amount_from_log(&log) {
                                return Ok(Some(amount));
                            }
                        }
                    }
                }

                Ok(None)
            }
            Err(e) => {
                log::debug!("Failed to get transaction {}: {}", signature, e);
                Ok(None)
            }
        }
    }

    /// Extracts numerical amounts from transaction log messages.
    ///
    /// # Params
    /// log - The log message to parse
    ///
    /// # Returns
    /// Returns the extracted amount if found, None otherwise
    ///
    /// # Example
    /// ```
    /// use orca_client::OrcaClient;
    ///
    /// let amount = OrcaClient::extract_amount_from_log("amount: 1500000");
    /// assert_eq!(amount, Some(1500000));
    /// ```
    fn extract_amount_from_log(log: &str) -> Option<u64> {
        let words: Vec<&str> = log
            .split(|c: char| c.is_whitespace() || c == ':' || c == '=' || c == ',')
            .collect();
        for (i, word) in words.iter().enumerate() {
            let lower_word = word.to_lowercase();
            if lower_word.contains("amount")
                || lower_word.contains("input")
                || lower_word.contains("output")
                || lower_word.contains("swap")
                || lower_word.contains("transfer")
            {
                for j in (i + 1)..words.len().min(i + 4) {
                    if let Some(amount) = Self::parse_possible_number(words[j]) {
                        if amount > 100 {
                            return Some(amount);
                        }
                    }
                }
            }
            if let Some(amount) = Self::parse_possible_number(word) {
                if amount > 1000 && amount < 1_000_000_000 {
                    return Some(amount);
                }
            }
        }
        None
    }

    /// Parses numeric values from strings, filtering out non-digit characters.
    ///
    /// # Params
    /// s - String that may contain a number
    ///
    /// # Returns
    /// Returns the parsed number if successful, None otherwise
    ///
    /// # Example
    /// ```
    /// use orca_client::OrcaClient;
    ///
    /// let number = OrcaClient::parse_possible_number("123abc");
    /// assert_eq!(number, Some(123));
    ///
    /// let invalid = OrcaClient::parse_possible_number("abc");
    /// assert_eq!(invalid, None);
    /// ```
    fn parse_possible_number(s: &str) -> Option<u64> {
        let cleaned: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !cleaned.is_empty() {
            cleaned.parse::<u64>().ok()
        } else {
            None
        }
    }

    /// Calculates a comprehensive health score for a liquidity pool.
    ///
    /// Uses logarithmic scaling to normalize metrics and weighted averaging for final score.
    ///
    /// # Params
    /// liquidity - Total liquidity in the pool
    /// volume - 24-hour trading volume
    /// fee_growth - Total fee growth
    ///
    /// # Example
    /// ```rust
    /// use orca_client::OrcaClient;
    ///
    /// let client = OrcaClient::new();
    /// let score = client.calculate_health_score(1_000_000, 500_000, 100_000);
    /// assert!(score >= 0.0 && score <= 100.0);
    /// ```
    fn calculate_health_score(&self, liquidity: u128, volume: u64, fee_growth: u128) -> f64 {
        let liquidity_score = (liquidity as f64 / 1e6).ln_1p().min(10.0);
        let volume_score = (volume as f64 / 1e3).ln_1p().min(10.0);
        let fee_score = (fee_growth as f64 / 1e6).ln_1p().min(10.0);
        (liquidity_score * 0.5 + volume_score * 0.3 + fee_score * 0.2) * 10.0
    }
}

#[derive(Debug, Clone)]
pub struct PoolHealth {
    pub liquidity: u128,
    pub volume_24h: u64,
    pub fee_growth: u128,
    pub health_score: f64,
}
