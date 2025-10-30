use solana_account_decoder::UiAccountEncoding;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::RpcFilterType;
use solana_commitment_config::CommitmentConfig;

use super::*;
use crate::global::*;
use crate::types::OrcaResult;

#[derive(Debug, Clone)]
pub struct PoolInfo {
    pub address: String,
    pub token_mint_a: String,
    pub token_mint_b: String,
    pub token_vault_a: String,
    pub token_vault_b: String,
    pub lp_token_mint: String,
    pub fee_account: String,
    pub trade_fee_numerator: u64,
    pub trade_fee_denominator: u64,
    pub tick_spacing: u16,
    pub liquidity: u128,
    pub sqrt_price: u128,
    pub fee_growth_global_a: u128,
    pub fee_growth_global_b: u128,
}

#[derive(Debug, Clone)]
pub struct QuoteResult {
    pub input_amount: u64,
    pub output_amount: u64,
    pub min_output_amount: u64,
    pub price_impact: f64,
    pub fee_amount: u64,
}

impl OrcaClient {
    /// Fetches pool state from on-chain data
    ///
    /// # Example
    /// ```
    /// let pool_info = client.get_pool_state_onchain("address").await?;
    /// println!("Pool liquidity: {}", pool_info.liquidity);
    /// ```
    pub async fn get_pool_state_onchain(&self, pool_address: &str) -> OrcaResult<PoolInfo> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let pool_pubkey = Pubkey::from_str(pool_address)
            .map_err(|e| OrcaError::Error(format!("Invalid pool address: {}", e)))?;
        let account_data = client
            .get_account_data(&pool_pubkey)
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get account data: {}", e)))?;
        self.parse_whirlpool_account_data(&account_data, pool_address)
    }

    /// Parses Whirlpool account data into PoolInfo struct
    fn parse_whirlpool_account_data(
        &self,
        data: &[u8],
        pool_address: &str,
    ) -> OrcaResult<PoolInfo> {
        if data.len() < 300 {
            return Err(OrcaError::Error(
                "Invalid whirlpool account data length".to_string(),
            ));
        }
        let token_mint_a = Pubkey::new_from_array(
            data[WHIRLPOOL_TOKEN_MINT_A_OFFSET..WHIRLPOOL_TOKEN_MINT_A_OFFSET + 32]
                .try_into()
                .map_err(|_| OrcaError::Error("Failed to parse token mint A".to_string()))?,
        )
        .to_string();
        let token_mint_b = Pubkey::new_from_array(
            data[WHIRLPOOL_TOKEN_MINT_B_OFFSET..WHIRLPOOL_TOKEN_MINT_B_OFFSET + 32]
                .try_into()
                .map_err(|_| OrcaError::Error("Failed to parse token mint B".to_string()))?,
        )
        .to_string();
        let tick_spacing = u16::from_le_bytes(
            data[WHIRLPOOL_TICK_SPACING_OFFSET..WHIRLPOOL_TICK_SPACING_OFFSET + 2]
                .try_into()
                .map_err(|_| OrcaError::Error("Failed to parse tick spacing".to_string()))?,
        );
        let fee_rate = u16::from_le_bytes(
            data[WHIRLPOOL_FEE_RATE_OFFSET..WHIRLPOOL_FEE_RATE_OFFSET + 2]
                .try_into()
                .map_err(|_| OrcaError::Error("Failed to parse fee rate".to_string()))?,
        );
        let liquidity = u128::from_le_bytes(
            data[WHIRLPOOL_LIQUIDITY_OFFSET..WHIRLPOOL_LIQUIDITY_OFFSET + 16]
                .try_into()
                .map_err(|_| OrcaError::Error("Failed to parse liquidity".to_string()))?,
        );
        let sqrt_price = u128::from_le_bytes(
            data[WHIRLPOOL_SQRT_PRICE_OFFSET..WHIRLPOOL_SQRT_PRICE_OFFSET + 16]
                .try_into()
                .map_err(|_| OrcaError::Error("Failed to parse sqrt price".to_string()))?,
        );
        let token_vault_a = self.derive_token_vault_address(&token_mint_a, pool_address)?;
        let token_vault_b = self.derive_token_vault_address(&token_mint_b, pool_address)?;
        let lp_token_mint = self.derive_lp_token_mint(pool_address)?;
        let fee_account = self.derive_fee_account(pool_address)?;
        let fee_growth_global_a = if data.len() >= 248 {
            u128::from_le_bytes(data[232..248].try_into().unwrap_or([0; 16]))
        } else {
            0
        };
        let fee_growth_global_b = if data.len() >= 264 {
            u128::from_le_bytes(data[248..264].try_into().unwrap_or([0; 16]))
        } else {
            0
        };
        Ok(PoolInfo {
            address: pool_address.to_string(),
            token_mint_a,
            token_mint_b,
            token_vault_a,
            token_vault_b,
            lp_token_mint,
            fee_account,
            trade_fee_numerator: fee_rate as u64,
            trade_fee_denominator: 1_000_000,
            tick_spacing,
            liquidity,
            sqrt_price,
            fee_growth_global_a,
            fee_growth_global_b,
        })
    }

    /// Derives token vault address using PDA
    fn derive_token_vault_address(
        &self,
        token_mint: &str,
        pool_address: &str,
    ) -> OrcaResult<String> {
        let token_mint_pubkey = Pubkey::from_str(token_mint)
            .map_err(|e| OrcaError::Error(format!("Invalid token mint: {}", e)))?;
        let pool_pubkey = Pubkey::from_str(pool_address)
            .map_err(|e| OrcaError::Error(format!("Invalid pool address: {}", e)))?;
        let (vault_address, _) = Pubkey::find_program_address(
            &[
                b"token_vault",
                pool_pubkey.as_ref(),
                token_mint_pubkey.as_ref(),
            ],
            &self.whirlpool_program_id,
        );
        Ok(vault_address.to_string())
    }

    /// Derives LP token mint address using PDA
    fn derive_lp_token_mint(&self, pool_address: &str) -> OrcaResult<String> {
        let pool_pubkey = Pubkey::from_str(pool_address)
            .map_err(|e| OrcaError::Error(format!("Invalid pool address: {}", e)))?;
        let (lp_mint, _) = Pubkey::find_program_address(
            &[b"lp_mint", pool_pubkey.as_ref()],
            &self.whirlpool_program_id,
        );
        Ok(lp_mint.to_string())
    }

    /// Derives fee account address using PDA
    fn derive_fee_account(&self, pool_address: &str) -> OrcaResult<String> {
        let pool_pubkey = Pubkey::from_str(pool_address)
            .map_err(|e| OrcaError::Error(format!("Invalid pool address: {}", e)))?;
        let (fee_account, _) = Pubkey::find_program_address(
            &[b"fee_account", pool_pubkey.as_ref()],
            &self.whirlpool_program_id,
        );
        Ok(fee_account.to_string())
    }

    /// Optimized method to find pools containing a specific token
    ///
    /// # Example
    /// ```
    /// let pools = client.find_pools_by_token_onchain_optimized("So11111111111111111111111111111111111111112").await?;
    /// println!("Found {} pools", pools.len());
    /// ```
    pub async fn find_pools_by_token_onchain_optimized(
        &self,
        token_mint: &str,
    ) -> OrcaResult<Vec<String>> {
        if let Some(cached_pools) = self.get_cached_pools_for_token(token_mint).await? {
            return Ok(cached_pools);
        }
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let token_pubkey = Pubkey::from_str(token_mint)
            .map_err(|e| OrcaError::Error(format!("Invalid token mint: {}", e)))?;
        let filters = vec![RpcFilterType::DataSize(300)];
        let accounts = client
            .get_program_accounts_with_config(
                &self.whirlpool_program_id,
                RpcProgramAccountsConfig {
                    filters: Some(filters),
                    account_config: RpcAccountInfoConfig {
                        encoding: Some(UiAccountEncoding::Base64),
                        data_slice: None,
                        commitment: Some(CommitmentConfig::confirmed()),
                        min_context_slot: None,
                    },
                    with_context: None,
                    sort_results: None,
                },
            )
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get program accounts: {}", e)))?;
        let mut pool_addresses = Vec::new();
        for (pubkey, account) in accounts {
            if account.data.len() < WHIRLPOOL_TOKEN_MINT_B_OFFSET + 32 {
                continue;
            }
            let mint_a_bytes: [u8; 32] = account.data
                [WHIRLPOOL_TOKEN_MINT_A_OFFSET..WHIRLPOOL_TOKEN_MINT_A_OFFSET + 32]
                .try_into()
                .map_err(|_| OrcaError::Error("Failed to convert mint A bytes".to_string()))?;
            let mint_b_bytes: [u8; 32] = account.data
                [WHIRLPOOL_TOKEN_MINT_B_OFFSET..WHIRLPOOL_TOKEN_MINT_B_OFFSET + 32]
                .try_into()
                .map_err(|_| OrcaError::Error("Failed to convert mint B bytes".to_string()))?;
            let mint_a = Pubkey::new_from_array(mint_a_bytes);
            let mint_b = Pubkey::new_from_array(mint_b_bytes);
            if mint_a == token_pubkey || mint_b == token_pubkey {
                pool_addresses.push(pubkey.to_string());
            }
        }
        self.cache_pools_for_token(token_mint, &pool_addresses)
            .await?;
        Ok(pool_addresses)
    }

    /// Retrieves cached pools for a token
    async fn get_cached_pools_for_token(
        &self,
        token_mint: &str,
    ) -> OrcaResult<Option<Vec<String>>> {
        todo!();
        Ok(None)
    }

    async fn cache_pools_for_token(&self, token_mint: &str, pools: &[String]) -> OrcaResult<()> {
        todo!();
        Ok(())
    }

    pub async fn find_pools_by_token_onchain(&self, token_mint: &str) -> OrcaResult<Vec<String>> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let token_pubkey = Pubkey::from_str(token_mint)
            .map_err(|e| OrcaError::Error(format!("Invalid token mint: {}", e)))?;
        let filters = vec![
            solana_client::rpc_filter::RpcFilterType::Memcmp(
                solana_client::rpc_filter::Memcmp::new_base58_encoded(
                    WHIRLPOOL_TOKEN_MINT_A_OFFSET,
                    &token_pubkey.to_bytes(),
                ),
            ),
            solana_client::rpc_filter::RpcFilterType::Memcmp(
                solana_client::rpc_filter::Memcmp::new_base58_encoded(
                    WHIRLPOOL_TOKEN_MINT_B_OFFSET,
                    &token_pubkey.to_bytes(),
                ),
            ),
        ];
        let accounts = client
            .get_program_accounts_with_config(
                &self.whirlpool_program_id,
                solana_client::rpc_config::RpcProgramAccountsConfig {
                    filters: Some(filters),
                    account_config: RpcAccountInfoConfig {
                        encoding: Some(UiAccountEncoding::Base64),
                        data_slice: None,
                        commitment: Some(CommitmentConfig::confirmed()),
                        min_context_slot: None,
                    },
                    with_context: None,
                    sort_results: None,
                },
            )
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get program accounts: {}", e)))?;
        let pool_addresses: Vec<String> = accounts
            .iter()
            .map(|(pubkey, _account)| pubkey.to_string())
            .collect();
        Ok(pool_addresses)
    }

    /// Gets a quote for swapping between two tokens
    ///
    /// # Example
    /// ```
    /// let quote = client.get_quote_from_pool(
    ///     "So11111111111111111111111111111111111111112",
    ///     "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    ///     1000000,
    ///     0.5
    /// ).await?;
    /// println!("Output amount: {}", quote.output_amount);
    /// ```
    pub async fn get_quote_from_pool(
        &self,
        input_mint: &str,
        output_mint: &str,
        input_amount: u64,
        slippage: f64,
    ) -> OrcaResult<QuoteResult> {
        let pools = self.find_pools_by_token_onchain(input_mint).await?;
        for pool_address in pools {
            if let Ok(pool_info) = self.get_pool_state_onchain(&pool_address).await {
                if (pool_info.token_mint_a == input_mint && pool_info.token_mint_b == output_mint)
                    || (pool_info.token_mint_a == output_mint
                        && pool_info.token_mint_b == input_mint)
                {
                    return self
                        .calculate_quote_from_pool_state(
                            &pool_info,
                            input_mint,
                            output_mint,
                            input_amount,
                            slippage,
                        )
                        .await;
                }
            }
        }
        Err(OrcaError::Error("No pool found for token pair".to_string()))
    }

    async fn calculate_quote_from_pool_state(
        &self,
        pool: &PoolInfo,
        input_mint: &str,
        output_mint: &str,
        input_amount: u64,
        slippage: f64,
    ) -> OrcaResult<QuoteResult> {
        let is_input_a = input_mint == pool.token_mint_a;
        let sqrt_price = pool.sqrt_price as f64;
        let scale_factor = 2f64.powi(64);
        let price = (sqrt_price * sqrt_price) / scale_factor;
        let output_amount = if is_input_a {
            (input_amount as f64 * price) as u64
        } else {
            (input_amount as f64 / price) as u64
        };
        let fee_amount = (input_amount as f64
            * (pool.trade_fee_numerator as f64 / pool.trade_fee_denominator as f64))
            as u64;
        let min_output_amount = (output_amount as f64 * (1.0 - slippage / 100.0)) as u64;
        let price_impact = self
            .calculate_price_impact(pool, input_amount, is_input_a)
            .await?;
        Ok(QuoteResult {
            input_amount,
            output_amount,
            min_output_amount,
            price_impact,
            fee_amount,
        })
    }

    async fn calculate_price_impact(
        &self,
        pool: &PoolInfo,
        input_amount: u64,
        is_input_a: bool,
    ) -> OrcaResult<f64> {
        let liquidity = pool.liquidity as f64;
        let impact = (input_amount as f64) / liquidity * 100.0;
        Ok(impact.min(100.0))
    }

    pub async fn derive_price_from_pool_state(
        &self,
        pool: &PoolInfo,
        base_mint: &str,
    ) -> OrcaResult<f64> {
        let sqrt_price = pool.sqrt_price as f64;
        let scale_factor = 2f64.powi(64);
        let price = (sqrt_price * sqrt_price) / scale_factor;
        if base_mint == pool.token_mint_a {
            Ok(price)
        } else {
            Ok(1.0 / price)
        }
    }

    /// Gets all pools containing a specific token from on-chain data
    pub async fn get_pools_by_token_onchain(&self, token_mint: &str) -> OrcaResult<Vec<String>> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let token_pubkey = Pubkey::from_str(token_mint)
            .map_err(|e| OrcaError::Error(format!("Invalid token mint: {}", e)))?;
        let accounts = client
            .get_program_accounts(&self.whirlpool_program_id)
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get program accounts: {}", e)))?;
        let mut pool_addresses = Vec::new();
        for (pubkey, account) in accounts {
            if account.data.len() < crate::global::WHIRLPOOL_TOKEN_MINT_B_OFFSET + 32 {
                continue;
            }
            let mint_a_bytes: [u8; 32] = account.data[crate::global::WHIRLPOOL_TOKEN_MINT_A_OFFSET
                ..crate::global::WHIRLPOOL_TOKEN_MINT_A_OFFSET + 32]
                .try_into()
                .map_err(|_| OrcaError::Error("Failed to convert mint A bytes".to_string()))?;
            let mint_b_bytes: [u8; 32] = account.data[crate::global::WHIRLPOOL_TOKEN_MINT_B_OFFSET
                ..crate::global::WHIRLPOOL_TOKEN_MINT_B_OFFSET + 32]
                .try_into()
                .map_err(|_| OrcaError::Error("Failed to convert mint B bytes".to_string()))?;
            let mint_a = Pubkey::new_from_array(mint_a_bytes);
            let mint_b = Pubkey::new_from_array(mint_b_bytes);
            if mint_a == token_pubkey || mint_b == token_pubkey {
                pool_addresses.push(pubkey.to_string());
            }
        }
        Ok(pool_addresses)
    }
}
