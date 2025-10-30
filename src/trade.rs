use super::*;
use crate::types::OrcaResult;
use solana_sdk::message::{AccountMeta, Instruction};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct TradeConfig {
    pub slippage: f64,
    pub max_iterations: u8,
}

impl Default for TradeConfig {
    fn default() -> Self {
        Self {
            slippage: 0.5,
            max_iterations: 3,
        }
    }
}

impl OrcaClient {
    /// Executes a token swap between specified input and output mints
    ///
    /// # Arguments
    /// keypair - Keypair for signing the transaction
    /// input_mint - Mint address of the input token
    /// output_mint - Mint address of the output token
    /// amount - Amount of input tokens to swap
    /// config - Optional trade configuration parameters
    ///
    /// # Returns
    /// Transaction signature if successful
    ///
    /// # Examples
    /// ```rust
    /// use orca_sdk::client::OrcaClient;
    /// use solana_sdk::signature::Keypair;
    ///
    /// let client = OrcaClient::new();
    /// let keypair = Keypair::new();
    /// let input_mint = "So11111111111111111111111111111111111111112";
    /// let output_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    /// let amount = 1_000_000; // 1 SOL
    ///
    /// let signature = client.swap(&keypair, input_mint, output_mint, amount, None).await?;
    /// println!("Swap completed with signature: {}", signature);
    /// ```
    pub async fn swap(
        &self,
        keypair: &Keypair,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        config: Option<TradeConfig>,
    ) -> OrcaResult<Signature> {
        let config = config.unwrap_or_default();
        let quote = self
            .get_quote_from_pool(input_mint, output_mint, amount, config.slippage)
            .await?;
        let input_mint_pubkey = Pubkey::from_str(input_mint)
            .map_err(|e| OrcaError::Error(format!("Invalid input mint: {}", e)))?;
        let output_mint_pubkey = Pubkey::from_str(output_mint)
            .map_err(|e| OrcaError::Error(format!("Invalid output mint: {}", e)))?;
        let input_token_account = self
            .ensure_token_account(keypair, &input_mint_pubkey)
            .await?;
        let output_token_account = self
            .ensure_token_account(keypair, &output_mint_pubkey)
            .await?;
        let pools = self.find_pools_by_token_onchain(input_mint).await?;
        let target_pool = {
            let mut found_pool = None;
            for pool in pools {
                if let Ok(pool_info) = self.get_pool_state_onchain(&pool).await {
                    if (pool_info.token_mint_a == input_mint
                        && pool_info.token_mint_b == output_mint)
                        || (pool_info.token_mint_a == output_mint
                            && pool_info.token_mint_b == input_mint)
                    {
                        found_pool = Some(pool.clone());
                        break;
                    }
                }
            }
            found_pool
        }
        .ok_or(OrcaError::Error("No suitable pool found".to_string()))?;
        let pool_pubkey = Pubkey::from_str(&target_pool)
            .map_err(|e| OrcaError::Error(format!("Invalid pool address: {}", e)))?;
        let recent_blockhash = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .get_latest_blockhash()
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get blockhash: {}", e)))?;
        let swap_instruction = self.build_swap_instruction(
            &keypair.pubkey(),
            &pool_pubkey,
            &input_token_account,
            &output_token_account,
            &input_mint_pubkey,
            &output_mint_pubkey,
            amount,
            quote.min_output_amount,
        )?;
        let message = Message::new(&[swap_instruction], Some(&keypair.pubkey()));
        let transaction = Transaction::new(&[keypair], message, recent_blockhash);
        self.solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .send_and_confirm_transaction(&transaction)
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to execute swap: {}", e)))
    }

    /// Constructs a swap instruction for the Whirlpool program
    ///
    /// # Arguments
    /// owner - Owner of the token accounts
    /// pool - Whirlpool address
    /// input_token_account - Input token account
    /// output_token_account - Output token account
    /// input_mint - Input token mint
    /// output_mint - Output token mint
    /// input_amount - Amount of input tokens
    /// min_output_amount - Minimum amount of output tokens to receive
    ///
    /// # Examples
    /// ```rust
    /// use orca_sdk::client::OrcaClient;
    /// use solana_sdk::pubkey::Pubkey;
    ///
    /// let client = OrcaClient::new_with_defaults();
    /// let owner = Pubkey::new_unique();
    /// let pool = Pubkey::new_unique();
    /// let input_token_account = Pubkey::new_unique();
    /// let output_token_account = Pubkey::new_unique();
    /// let input_mint = Pubkey::new_unique();
    /// let output_mint = Pubkey::new_unique();
    /// let input_amount = 1_000_000;
    /// let min_output_amount = 500_000;
    ///
    /// let instruction = client.build_swap_instruction(
    ///     &owner,
    ///     &pool,
    ///     &input_token_account,
    ///     &output_token_account,
    ///     &input_mint,
    ///     &output_mint,
    ///     input_amount,
    ///     min_output_amount,
    /// )?;
    /// ```
    fn build_swap_instruction(
        &self,
        owner: &Pubkey,
        pool: &Pubkey,
        input_token_account: &Pubkey,
        output_token_account: &Pubkey,
        input_mint: &Pubkey,
        output_mint: &Pubkey,
        input_amount: u64,
        min_output_amount: u64,
    ) -> OrcaResult<Instruction> {
        let token_vault_a = self.get_associated_token_address(pool, input_mint);
        let token_vault_b = self.get_associated_token_address(pool, output_mint);
        let accounts = vec![
            AccountMeta::new_readonly(self.whirlpool_program_id, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*pool, false),
            AccountMeta::new(*input_token_account, false),
            AccountMeta::new(*output_token_account, false),
            AccountMeta::new(token_vault_a, false),
            AccountMeta::new(token_vault_b, false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ];
        let mut data = vec![0x01]; // swap instruction discriminator
        data.extend_from_slice(&input_amount.to_le_bytes());
        data.extend_from_slice(&min_output_amount.to_le_bytes());
        Ok(Instruction {
            program_id: self.whirlpool_program_id,
            accounts,
            data,
        })
    }
}
