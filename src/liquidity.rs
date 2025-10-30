use super::*;
use crate::{pool::PoolInfo, types::OrcaResult};
use solana_program::example_mocks::solana_sdk::system_program;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    sysvar,
};
use std::str::FromStr;

/// Represents a liquidity position in a concentrated liquidity pool
#[derive(Debug, Clone)]
pub struct LiquidityPosition {
    pub pool_address: Pubkey,
    pub token_a_amount: u64,
    pub token_b_amount: u64,
    pub lp_token_amount: u64,
    pub lower_tick: i32,
    pub upper_tick: i32,
    pub position_mint: Pubkey,
    pub position_token_account: Pubkey,
}

/// Configuration for adding liquidity with slippage protection
#[derive(Debug, Clone)]
pub struct AddLiquidityConfig {
    pub slippage_tolerance: f64,
    pub max_iterations: u8,
}

impl Default for AddLiquidityConfig {
    fn default() -> Self {
        Self {
            slippage_tolerance: 0.5,
            max_iterations: 3,
        }
    }
}

impl OrcaClient {
    /// Adds liquidity to a concentrated liquidity pool within specified tick range
    ///
    /// # Params
    /// keypair - Keypair for transaction signing
    /// pool - Pool information
    /// token_a_amount - Amount of token A to deposit
    /// token_b_amount - Amount of token B to deposit  
    /// lower_tick - Lower tick boundary for position
    /// upper_tick - Upper tick boundary for position
    /// config - Optional configuration for slippage and iterations
    ///
    /// # Example
    /// ```rust
    /// use orca_rs::client::OrcaClient;
    /// use solana_sdk::signature::Keypair;
    ///
    /// let client = OrcaClient::new("https://api.mainnet-beta.solana.com");
    /// let keypair = Keypair::new();
    /// let pool_info = client.get_pool("whirlpool_address").await?;
    ///
    /// let signature = client.add_liquidity(
    ///     &keypair,
    ///     &pool_info,
    ///     1000000, // 1 token A
    ///     2000000, // 2 token B  
    ///     -1000,   // lower tick
    ///     1000,    // upper tick
    ///     None,    // use default config
    /// ).await?;
    /// ```
    pub async fn add_liquidity(
        &self,
        keypair: &Keypair,
        pool: &PoolInfo,
        token_a_amount: u64,
        token_b_amount: u64,
        lower_tick: i32,
        upper_tick: i32,
        config: Option<AddLiquidityConfig>,
    ) -> OrcaResult<Signature> {
        let token_a_mint = Pubkey::from_str(&pool.token_mint_a)
            .map_err(|e| OrcaError::Error(format!("Invalid token mint A: {}", e)))?;
        let token_b_mint = Pubkey::from_str(&pool.token_mint_b)
            .map_err(|e| OrcaError::Error(format!("Invalid token mint B: {}", e)))?;
        let token_a_account = self.ensure_token_account(keypair, &token_a_mint).await?;
        let token_b_account = self.ensure_token_account(keypair, &token_b_mint).await?;
        let pool_pubkey = Pubkey::from_str(&pool.address)
            .map_err(|e| OrcaError::Error(format!("Invalid pool address: {}", e)))?;
        let recent_blockhash = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .get_latest_blockhash()
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get blockhash: {}", e)))?;
        let position_mint = Keypair::new();
        let position_token_account =
            self.get_associated_token_address(&keypair.pubkey(), &position_mint.pubkey());
        let open_position_instruction = self.build_open_position_instruction(
            &keypair.pubkey(),
            &pool_pubkey,
            &position_mint.pubkey(),
            &position_token_account,
            lower_tick,
            upper_tick,
        )?;
        let increase_liquidity_instruction = self.build_increase_liquidity_instruction(
            &keypair.pubkey(),
            &pool_pubkey,
            &position_token_account,
            &token_a_account,
            &token_b_account,
            &token_a_mint,
            &token_b_mint,
            &position_mint.pubkey(),
            token_a_amount,
            token_b_amount,
        )?;
        let message = Message::new(
            &[open_position_instruction, increase_liquidity_instruction],
            Some(&keypair.pubkey()),
        );
        let transaction = Transaction::new(&[keypair, &position_mint], message, recent_blockhash);
        self.solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .send_and_confirm_transaction(&transaction)
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to add liquidity: {}", e)))
    }

    /// Removes liquidity from a position and closes it
    ///
    /// # Params
    /// keypair - Keypair for transaction signing
    /// position - Liquidity position to remove
    ///
    /// # Example
    /// ```rust
    /// use orca_rs::client::OrcaClient;
    /// use solana_sdk::signature::Keypair;
    ///
    /// let client = OrcaClient::new("https://api.mainnet-beta.solana.com");
    /// let keypair = Keypair::new();
    /// let positions = client.get_liquidity_positions(&keypair.pubkey()).await?;
    ///
    /// if let Some(position) = positions.first() {
    ///     let signature = client.remove_liquidity(&keypair, position).await?;
    /// }
    /// ```
    pub async fn remove_liquidity(
        &self,
        keypair: &Keypair,
        position: &LiquidityPosition,
    ) -> OrcaResult<Signature> {
        let recent_blockhash = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .get_latest_blockhash()
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get blockhash: {}", e)))?;
        let decrease_liquidity_instruction = self.build_decrease_liquidity_instruction(
            &keypair.pubkey(),
            &position.pool_address,
            &position.position_token_account,
            &position.position_mint,
            position.lp_token_amount,
        )?;
        let close_position_instruction = self.build_close_position_instruction(
            &keypair.pubkey(),
            &position.pool_address,
            &position.position_token_account,
            &position.position_mint,
        )?;
        let message = Message::new(
            &[decrease_liquidity_instruction, close_position_instruction],
            Some(&keypair.pubkey()),
        );
        let transaction = Transaction::new(&[keypair], message, recent_blockhash);
        self.solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .send_and_confirm_transaction(&transaction)
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to remove liquidity: {}", e)))
    }

    /// Retrieves all liquidity positions for a given owner
    ///
    /// # Params
    /// owner - Public key of the position owner
    ///
    /// # Example
    /// ```rust
    /// use orca_rs::client::OrcaClient;
    /// use solana_sdk::pubkey::Pubkey;
    ///
    /// let client = OrcaClient::new("https://api.mainnet-beta.solana.com");
    /// let owner = Pubkey::new_unique();
    /// let positions = client.get_liquidity_positions(&owner).await?;
    ///
    /// for position in positions {
    ///     println!("Position: {} LP tokens", position.lp_token_amount);
    /// }
    /// ```
    pub async fn get_liquidity_positions(
        &self,
        owner: &Pubkey,
    ) -> OrcaResult<Vec<LiquidityPosition>> {
        let token_accounts = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .get_token_accounts_by_owner(
                owner,
                solana_client::rpc_request::TokenAccountsFilter::ProgramId(spl_token::id()),
            )
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get token accounts: {}", e)))?;
        let mut positions = Vec::new();
        for account in token_accounts {
            let account_data_bytes = self.decode_account_data(&account.account.data)?;
            let token_account = spl_token::state::Account::unpack_from_slice(&account_data_bytes)
                .map_err(|e| {
                OrcaError::Error(format!("Failed to unpack token account: {}", e))
            })?;
            if token_account.amount > 0 && self.is_position_token(&token_account.mint).await? {
                let position = LiquidityPosition {
                    pool_address: Pubkey::default(), // 需要从链上数据解析
                    token_a_amount: 0,
                    token_b_amount: 0,
                    lp_token_amount: token_account.amount,
                    lower_tick: 0,
                    upper_tick: 0,
                    position_mint: token_account.mint,
                    position_token_account: Pubkey::from_str(&account.pubkey)
                        .map_err(|e| OrcaError::Error(format!("Invalid account pubkey: {}", e)))?,
                };
                positions.push(position);
            }
        }
        Ok(positions)
    }

    async fn is_position_token(&self, mint: &Pubkey) -> OrcaResult<bool> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let mint_account = match client.get_account(mint).await {
            Ok(account) => account,
            Err(_) => return Ok(false),
        };
        let token_data = spl_token::state::Mint::unpack(&mint_account.data)
            .map_err(|e| OrcaError::Error(format!("Failed to unpack mint data: {}", e)))?;
        if self
            .is_position_token_by_metadata(mint, &token_data)
            .await?
        {
            return Ok(true);
        }
        if self.is_position_token_by_holders(mint).await? {
            return Ok(true);
        }
        if self.is_position_token_by_pool_association(mint).await? {
            return Ok(true);
        }
        Ok(false)
    }

    async fn is_position_token_by_metadata(
        &self,
        mint: &Pubkey,
        token_data: &spl_token::state::Mint,
    ) -> OrcaResult<bool> {
        let token_name = self.get_token_name(mint).await.unwrap_or_default();
        let token_symbol = self.get_token_symbol(mint).await.unwrap_or_default();
        let position_patterns = ["position", "LP", "liquidity", "whirlpool", "concentrated"];
        for pattern in &position_patterns {
            if token_name.to_lowercase().contains(pattern)
                || token_symbol.to_lowercase().contains(pattern)
            {
                return Ok(true);
            }
        }
        if token_data.decimals == 6 || token_data.decimals == 9 {
            return self.verify_position_token(mint).await;
        }
        Ok(false)
    }

    async fn is_position_token_by_holders(&self, mint: &Pubkey) -> OrcaResult<bool> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let token_accounts = client
            .get_token_accounts_by_owner(
                &self.whirlpool_program_id,
                solana_client::rpc_request::TokenAccountsFilter::Mint(*mint),
            )
            .await;
        if let Ok(accounts) = token_accounts {
            if !accounts.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn is_position_token_by_pool_association(&self, mint: &Pubkey) -> OrcaResult<bool> {
        let pools = self.get_all_whirlpools().await?;
        for pool in pools {
            if let Ok(pool_info) = self.get_pool_state_onchain(&pool).await {
                if pool_info.lp_token_mint == mint.to_string() {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    async fn get_all_whirlpools(&self) -> OrcaResult<Vec<String>> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let accounts = client
            .get_program_accounts(&self.whirlpool_program_id)
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get program accounts: {}", e)))?;
        Ok(accounts
            .into_iter()
            .map(|(pubkey, _)| pubkey.to_string())
            .collect())
    }

    /// Verifies if a token is a valid Whirlpool position token
    ///
    /// # Params
    /// mint - The position token mint address to verify
    ///
    /// # Example
    /// ```rust
    /// use orca_rs::client::OrcaClient;
    /// use solana_sdk::pubkey::Pubkey;
    ///
    /// let client = OrcaClient::new("https://api.mainnet-beta.solana.com");
    /// let mint = Pubkey::new_unique();
    /// let is_position = client.verify_position_token(&mint).await?;
    /// println!("Is position token: {}", is_position);
    /// ```
    async fn verify_position_token(&self, mint: &Pubkey) -> OrcaResult<bool> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        // Method 1: Check if there's a position account for this mint
        let position_pda = self.get_position_pda(mint);
        match client.get_account(&position_pda).await {
            Ok(account) => {
                // Verify the account is owned by whirlpool program and has data
                Ok(account.owner == self.whirlpool_program_id && account.data.len() >= 216) // Minimum position account size
            }
            Err(_) => Ok(false),
        }
    }

    /// Derives the position PDA from the position token mint
    fn get_position_pda(&self, position_mint: &Pubkey) -> Pubkey {
        let (pda, _) = Pubkey::find_program_address(
            &[b"position", position_mint.as_ref()],
            &self.whirlpool_program_id,
        );
        pda
    }

    async fn get_token_name(&self, mint: &Pubkey) -> OrcaResult<String> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let metadata_program = Pubkey::from_str(crate::global::TOKEN_METADATA_PROGRAM_ID)
            .map_err(|e| OrcaError::Error(format!("Invalid metadata program ID: {}", e)))?;
        let (metadata_address, _) = Pubkey::find_program_address(
            &[b"metadata", metadata_program.as_ref(), mint.as_ref()],
            &metadata_program,
        );
        match client.get_account(&metadata_address).await {
            Ok(account) => {
                if account.data.len() > 120 {
                    let name_data = &account.data[69..109];
                    let name = String::from_utf8_lossy(name_data)
                        .trim_end_matches('\0')
                        .to_string();
                    if !name.is_empty() {
                        return Ok(name);
                    }
                }
                Ok("Unknown".to_string())
            }
            Err(_) => Ok("Unknown".to_string()),
        }
    }

    async fn get_token_symbol(&self, mint: &Pubkey) -> OrcaResult<String> {
        let client = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?;
        let metadata_program = Pubkey::from_str(crate::global::TOKEN_METADATA_PROGRAM_ID)
            .map_err(|e| OrcaError::Error(format!("Invalid metadata program ID: {}", e)))?;
        let (metadata_address, _) = Pubkey::find_program_address(
            &[b"metadata", metadata_program.as_ref(), mint.as_ref()],
            &metadata_program,
        );
        match client.get_account(&metadata_address).await {
            Ok(account) => {
                if account.data.len() > 120 {
                    let symbol_data = &account.data[109..119];
                    let symbol = String::from_utf8_lossy(symbol_data)
                        .trim_end_matches('\0')
                        .to_string();
                    if !symbol.is_empty() {
                        return Ok(symbol);
                    }
                }
                Ok("UNK".to_string())
            }
            Err(_) => Ok("UNK".to_string()),
        }
    }

    fn build_open_position_instruction(
        &self,
        owner: &Pubkey,
        pool: &Pubkey,
        position_mint: &Pubkey,
        position_token_account: &Pubkey,
        lower_tick: i32,
        upper_tick: i32,
    ) -> OrcaResult<Instruction> {
        let accounts = vec![
            AccountMeta::new_readonly(self.whirlpool_program_id, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*position_mint, true),
            AccountMeta::new(*position_token_account, false),
            AccountMeta::new(*pool, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
        ];
        let mut data = vec![0x08]; // open_position instruction discriminator
        data.extend_from_slice(&lower_tick.to_le_bytes());
        data.extend_from_slice(&upper_tick.to_le_bytes());
        Ok(Instruction {
            program_id: self.whirlpool_program_id,
            accounts,
            data,
        })
    }

    fn build_increase_liquidity_instruction(
        &self,
        owner: &Pubkey,
        pool: &Pubkey,
        position_token_account: &Pubkey,
        token_a_account: &Pubkey,
        token_b_account: &Pubkey,
        token_a_mint: &Pubkey,
        token_b_mint: &Pubkey,
        position_mint: &Pubkey,
        token_a_amount: u64,
        token_b_amount: u64,
    ) -> OrcaResult<Instruction> {
        let token_vault_a = self.get_associated_token_address(pool, token_a_mint);
        let token_vault_b = self.get_associated_token_address(pool, token_b_mint);
        let accounts = vec![
            AccountMeta::new_readonly(self.whirlpool_program_id, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*position_token_account, false),
            AccountMeta::new(*pool, false),
            AccountMeta::new(*token_a_account, false),
            AccountMeta::new(*token_b_account, false),
            AccountMeta::new(token_vault_a, false),
            AccountMeta::new(token_vault_b, false),
            AccountMeta::new(*position_mint, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ];
        let mut data = vec![0x09]; // increase_liquidity instruction discriminator
        data.extend_from_slice(&token_a_amount.to_le_bytes());
        data.extend_from_slice(&token_b_amount.to_le_bytes());
        Ok(Instruction {
            program_id: self.whirlpool_program_id,
            accounts,
            data,
        })
    }

    fn build_decrease_liquidity_instruction(
        &self,
        owner: &Pubkey,
        pool: &Pubkey,
        position_token_account: &Pubkey,
        position_mint: &Pubkey,
        liquidity_amount: u64,
    ) -> OrcaResult<Instruction> {
        let accounts = vec![
            AccountMeta::new_readonly(self.whirlpool_program_id, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*position_token_account, false),
            AccountMeta::new(*pool, false),
            AccountMeta::new(*position_mint, false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ];
        let mut data = vec![0x0A]; // decrease_liquidity instruction discriminator
        data.extend_from_slice(&liquidity_amount.to_le_bytes());
        Ok(Instruction {
            program_id: self.whirlpool_program_id,
            accounts,
            data,
        })
    }

    fn build_close_position_instruction(
        &self,
        owner: &Pubkey,
        pool: &Pubkey,
        position_token_account: &Pubkey,
        position_mint: &Pubkey,
    ) -> OrcaResult<Instruction> {
        let accounts = vec![
            AccountMeta::new_readonly(self.whirlpool_program_id, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*position_token_account, false),
            AccountMeta::new(*pool, false),
            AccountMeta::new(*position_mint, false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ];
        let data = vec![0x0B]; // close_position instruction discriminator
        Ok(Instruction {
            program_id: self.whirlpool_program_id,
            accounts,
            data,
        })
    }
}
