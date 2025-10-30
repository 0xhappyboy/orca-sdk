use std::time::Duration;

use super::*;
use crate::{monitoring::PriceData, types::OrcaResult};
use base64::{Engine, prelude::BASE64_STANDARD};
use solana_transaction_status::{
    EncodedTransaction, UiInstruction, UiMessage, UiParsedInstruction, UiTransactionEncoding,
};

impl OrcaClient {
    /// Get token price from a liquidity pool
    ///
    /// # Arguments
    /// base_mint - Base token mint address
    /// quote_mint - Quote token mint address
    ///
    /// # Example
    /// ```rust
    /// let price = client.get_token_price_from_pool(
    ///     "So11111111111111111111111111111111111111112", // SOL
    ///     "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"  // USDC
    /// ).await?;
    /// println!("SOL/USDC price: {}", price);
    /// ```
    pub async fn get_token_price_from_pool(
        &self,
        base_mint: &str,
        quote_mint: &str,
    ) -> OrcaResult<f64> {
        let pools = self.get_pools_by_token_onchain(base_mint).await?;
        for pool_address in pools {
            if let Ok(pool_info) = self.get_pool_state_onchain(&pool_address).await {
                if (pool_info.token_mint_a == base_mint && pool_info.token_mint_b == quote_mint)
                    || (pool_info.token_mint_a == quote_mint && pool_info.token_mint_b == base_mint)
                {
                    return self
                        .derive_price_from_pool_state(&pool_info, base_mint)
                        .await;
                }
            }
        }
        Err(OrcaError::Error("No pool found for token pair".to_string()))
    }

    /// Get price history from on-chain transactions
    ///
    /// # Arguments
    /// pool_address - Pool address to get history for
    /// limit - Maximum number of price points to return
    ///
    /// # Example
    /// ```rust
    /// let price_history = client.get_price_history_from_chain(
    ///     "whirlpool_address_here",
    ///     100
    /// ).await?;
    /// for data in price_history {
    ///     println!("Time: {}, Price: {}", data.timestamp, data.price);
    /// }
    /// ```
    pub async fn get_price_history_from_chain(
        &self,
        pool_address: &str,
        limit: usize,
    ) -> OrcaResult<Vec<PriceData>> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let pool_pubkey = Pubkey::from_str(pool_address)
            .map_err(|e| OrcaError::Error(format!("Invalid pool address: {}", e)))?;
        let base_pool_info = self.get_pool_state_onchain(pool_address).await?;
        let base_liquidity = base_pool_info.liquidity;
        let signatures = client
            .get_signatures_for_address(&pool_pubkey)
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get signatures: {}", e)))?;
        let mut price_history = Vec::new();
        for sig_info in signatures.iter().take(limit) {
            let signature = Signature::from_str(&sig_info.signature)
                .map_err(|e| OrcaError::Error(format!("Invalid signature: {}", e)))?;
            if let Ok(transaction) = client
                .get_transaction(&signature, UiTransactionEncoding::Base64)
                .await
            {
                if let Some(block_time) = transaction.block_time {
                    if let Some(price) = self
                        .extract_price_from_transaction(&transaction.transaction.transaction)
                        .await
                    {
                        price_history.push(PriceData {
                            timestamp: block_time as u64,
                            price,
                            liquidity: base_liquidity,
                        });
                    }
                }
            }
        }
        Ok(price_history)
    }

    async fn extract_price_from_transaction(
        &self,
        transaction: &EncodedTransaction,
    ) -> Option<f64> {
        match transaction {
            EncodedTransaction::Json(encoded_tx) => {
                self.extract_price_from_message(&encoded_tx.message).await
            }
            _ => None,
        }
    }

    async fn extract_price_from_message(&self, message: &UiMessage) -> Option<f64> {
        match message {
            UiMessage::Parsed(parsed_msg) => {
                for instruction in &parsed_msg.instructions {
                    if let Some(price) = self
                        .analyze_instruction_for_price(instruction, message)
                        .await
                    {
                        return Some(price);
                    }
                }
            }
            UiMessage::Raw(raw_msg) => {
                for compiled_instruction in &raw_msg.instructions {
                    let instruction = solana_transaction_status::UiInstruction::Compiled(
                        compiled_instruction.clone(),
                    );
                    if let Some(price) = self
                        .analyze_instruction_for_price(&instruction, message)
                        .await
                    {
                        return Some(price);
                    }
                }
            }
        }
        None
    }

    async fn analyze_instruction_for_price(
        &self,
        instruction: &UiInstruction,
        message: &solana_transaction_status::UiMessage,
    ) -> Option<f64> {
        match instruction {
            UiInstruction::Parsed(parsed) => {
                if let Some(program_name) = Self::get_instruction_program(parsed) {
                    if program_name.contains("swap")
                        || program_name.contains("orca")
                        || program_name.contains("token")
                        || program_name.contains("amm")
                    {
                        if let Some(amounts) = Self::extract_token_amounts_from_instruction(parsed)
                        {
                            if amounts.len() >= 2 && amounts[0] > 0.0 {
                                return Some(amounts[1] / amounts[0]);
                            }
                        }
                    }
                }
            }
            UiInstruction::Compiled(compiled) => {
                if let Some(price) = self.analyze_compiled_instruction(compiled, message).await {
                    return Some(price);
                }
            }
        }
        None
    }

    fn get_instruction_program(instruction: &UiParsedInstruction) -> Option<String> {
        match instruction {
            UiParsedInstruction::Parsed(parsed) => Some(parsed.program.clone()),
            UiParsedInstruction::PartiallyDecoded(partial) => Some(partial.program_id.to_string()),
        }
    }

    fn extract_token_amounts_from_instruction(
        instruction: &solana_transaction_status::UiParsedInstruction,
    ) -> Option<Vec<f64>> {
        let mut amounts = Vec::new();
        match instruction {
            solana_transaction_status::UiParsedInstruction::Parsed(parsed) => {
                if let parsed_data = &parsed.parsed {
                    if let serde_json::Value::Object(map) = parsed_data {
                        for (key, value) in map {
                            if key.contains("amount")
                                || key.contains("token")
                                || key.contains("quantity")
                                || key.contains("value")
                                || key.contains("source")
                                || key.contains("destination")
                            {
                                if let Some(amount) = Self::parse_amount_from_value(value) {
                                    amounts.push(amount);
                                }
                            }
                        }
                    }
                }
            }
            solana_transaction_status::UiParsedInstruction::PartiallyDecoded(partial) => {
                // To be realized
                todo!();
            }
        }
        if amounts.is_empty() {
            None
        } else {
            Some(amounts)
        }
    }

    fn parse_amount_from_value(value: &serde_json::Value) -> Option<f64> {
        match value {
            serde_json::Value::Number(num) => num.as_f64(),
            serde_json::Value::String(s) => s.parse::<f64>().ok(),
            _ => None,
        }
    }

    async fn analyze_compiled_instruction(
        &self,
        compiled: &solana_transaction_status::UiCompiledInstruction,
        message: &solana_transaction_status::UiMessage,
    ) -> Option<f64> {
        let program_id = match message {
            solana_transaction_status::UiMessage::Parsed(parsed_msg) => {
                if let account_keys = &parsed_msg.account_keys {
                    if let Some(id) = account_keys.get(compiled.program_id_index as usize) {
                        id.pubkey.clone()
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            }
            solana_transaction_status::UiMessage::Raw(raw_msg) => {
                if let Some(account) = raw_msg.account_keys.get(compiled.program_id_index as usize)
                {
                    account.clone()
                } else {
                    return None;
                }
            }
        };
        let is_swap_program = program_id == crate::global::ORCA_WHIRLPOOLS_PROGRAM_ID
            || program_id == crate::global::ORCA_STABLE_SWAP_PROGRAM_ID
            || program_id == crate::global::ORCA_SWAP_PROGRAM_ID_V1
            || program_id == crate::global::ORCA_SWAP_PROGRAM_ID_V2;
        if !is_swap_program {
            return None;
        }
        if let data = &compiled.data {
            if let Ok(decoded) = BASE64_STANDARD.decode(data) {
                if decoded.len() >= 17 {
                    let amount_in_bytes: [u8; 8] = decoded[1..9].try_into().ok()?;
                    let amount_out_bytes: [u8; 8] = decoded[9..17].try_into().ok()?;
                    let amount_in = u64::from_le_bytes(amount_in_bytes);
                    let amount_out = u64::from_le_bytes(amount_out_bytes);

                    if amount_in > 0 && amount_out > 0 {
                        return Some(amount_out as f64 / amount_in as f64);
                    }
                }
            }
        }

        None
    }

    /// Calculate moving average price from on-chain data
    ///
    /// # Arguments
    /// pool_address - Pool address to calculate MA for
    /// period - Number of periods for moving average
    ///
    /// # Example
    /// ```rust
    /// let ma_20 = client.calculate_moving_average_from_chain(
    ///     "whirlpool_address_here",
    ///     20
    /// ).await?;
    /// println!("20-period moving average: {}", ma_20);
    /// ```
    pub async fn calculate_moving_average_from_chain(
        &self,
        pool_address: &str,
        period: usize,
    ) -> OrcaResult<f64> {
        let prices = self
            .get_price_history_from_chain(pool_address, period)
            .await?;
        if prices.is_empty() {
            return Err(OrcaError::Error("No price data available".to_string()));
        }
        let sum: f64 = prices.iter().map(|p| p.price).sum();
        let average = sum / prices.len() as f64;
        Ok(average)
    }

    pub async fn get_kline_data_production(
        &self,
        pool_address: &str,
        timeframe_minutes: u32,
        limit: usize,
    ) -> OrcaResult<Vec<Kline>> {
        const MAX_RETRIES: u32 = 3;
        if timeframe_minutes == 0 || timeframe_minutes > 1440 {
            return Err(OrcaError::Error(
                "Invalid timeframe: must be between 1 and 1440 minutes".to_string(),
            ));
        }
        if limit > 500 {
            return Err(OrcaError::Error(
                "Limit too large: maximum 500 candles".to_string(),
            ));
        }
        let mut retries = 0;
        loop {
            match self
                .try_get_kline_data(pool_address, timeframe_minutes, limit)
                .await
            {
                Ok(kline_data) => {
                    if kline_data.is_empty() {
                        log::warn!("No kline data available for pool: {}", pool_address);
                    }
                    return Ok(kline_data);
                }
                Err(e) if retries < MAX_RETRIES => {
                    retries += 1;
                    let backoff_ms = 1000 * 2u64.pow(retries - 1);
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }

    async fn try_get_kline_data(
        &self,
        pool_address: &str,
        timeframe_minutes: u32,
        limit: usize,
    ) -> OrcaResult<Vec<Kline>> {
        // 获取交易历史作为价格数据源
        let transactions_needed = limit * 5;
        let price_history = self
            .get_price_history_from_chain(pool_address, transactions_needed)
            .await?;
        if price_history.is_empty() {
            return Ok(Vec::new());
        }
        let mut sorted_history = price_history;
        sorted_history.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        let timeframe_seconds = (timeframe_minutes * 60) as u64;
        let mut klines = Vec::with_capacity(limit);
        let mut current_timeframe_start =
            sorted_history[0].timestamp / timeframe_seconds * timeframe_seconds;
        let mut current_kline: Option<Kline> = None;
        for price_data in sorted_history {
            let timeframe_start = price_data.timestamp / timeframe_seconds * timeframe_seconds;
            if timeframe_start != current_timeframe_start {
                if let Some(kline) = current_kline.take() {
                    klines.push(kline);
                    if klines.len() >= limit {
                        break;
                    }
                }
                current_timeframe_start = timeframe_start;
                current_kline = Some(Kline {
                    timestamp: timeframe_start,
                    open: price_data.price,
                    high: price_data.price,
                    low: price_data.price,
                    close: price_data.price,
                    volume: 1.0,
                });
            } else if let Some(ref mut kline) = current_kline {
                kline.high = kline.high.max(price_data.price);
                kline.low = kline.low.min(price_data.price);
                kline.close = price_data.price;
                kline.volume += 1.0;
            }
        }
        if let Some(kline) = current_kline {
            klines.push(kline);
        }
        Ok(klines)
    }
}

/// K Line data
#[derive(Debug, Clone)]
pub struct Kline {
    pub timestamp: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}
