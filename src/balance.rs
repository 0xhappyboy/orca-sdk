use super::*;
use crate::types::OrcaResult;
use base64::{Engine, prelude::BASE64_STANDARD};
use solana_account_decoder::{UiAccountData, UiAccountEncoding};
use solana_client::rpc_request::TokenAccountsFilter;
use solana_sdk::program_pack::Pack;

impl OrcaClient {
    /// Get the balance of a specific token for a given owner and mint
    ///
    /// # Params
    /// owner - The public key of the token account owner
    /// mint - The public key of the token mint
    ///
    /// # Example
    /// ```rust
    /// use solana_sdk::pubkey;
    ///
    /// # async fn example(client: &OrcaClient) -> OrcaResult<()> {
    /// let owner = pubkey!("OwnerPublicKeyHere");
    /// let mint = pubkey!("MintPublicKeyHere");
    /// let balance = client.get_token_balance(&owner, &mint).await?;
    /// println!("Token balance: {}", balance);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_token_balance(&self, owner: &Pubkey, mint: &Pubkey) -> OrcaResult<u64> {
        let token_accounts = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .get_token_accounts_by_owner(owner, TokenAccountsFilter::Mint(*mint))
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get token accounts: {}", e)))?;
        if let Some(account) = token_accounts.first() {
            let account_data_bytes = self.decode_account_data(&account.account.data)?;
            let account_data: spl_token::state::Account =
                spl_token::state::Account::unpack(&account_data_bytes).map_err(|e| {
                    OrcaError::Error(format!("Failed to unpack token account: {}", e))
                })?;
            Ok(account_data.amount)
        } else {
            Ok(0)
        }
    }

    /// Get balances for all tokens owned by a specific account
    ///
    /// # Params
    /// owner - The public key of the token account owner
    ///
    /// # Example
    /// ```rust
    /// use solana_sdk::pubkey;
    ///
    /// # async fn example(client: &OrcaClient) -> OrcaResult<()> {
    /// let owner = pubkey!("OwnerPublicKeyHere");
    /// let balances = client.get_all_token_balances(&owner).await?;
    /// for (mint, balance) in balances {
    ///     println!("Mint: {}, Balance: {}", mint, balance);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_all_token_balances(&self, owner: &Pubkey) -> OrcaResult<Vec<(Pubkey, u64)>> {
        let token_accounts = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .get_token_accounts_by_owner(owner, TokenAccountsFilter::ProgramId(spl_token::id()))
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get token accounts: {}", e)))?;
        let mut balances = Vec::new();
        for account in token_accounts {
            let account_data_bytes = self.decode_account_data(&account.account.data)?;
            let account_data: spl_token::state::Account =
                spl_token::state::Account::unpack(&account_data_bytes).map_err(|e| {
                    OrcaError::Error(format!("Failed to unpack token account: {}", e))
                })?;

            if account_data.amount > 0 {
                balances.push((account_data.mint, account_data.amount));
            }
        }
        Ok(balances)
    }

    /// Ensure a token account exists for the given keypair and mint
    /// Creates the account if it doesn't exist
    ///
    /// # Params
    /// keypair - The keypair that owns the token account
    /// mint - The public key of the token mint
    ///
    /// # Example
    /// ```rust
    /// use solana_sdk::{pubkey, signer::keypair::Keypair};
    ///
    /// # async fn example(client: &OrcaClient) -> OrcaResult<()> {
    /// let keypair = Keypair::new();
    /// let mint = pubkey!("MintPublicKeyHere");
    /// let token_account = client.ensure_token_account(&keypair, &mint).await?;
    /// println!("Token account: {}", token_account);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn ensure_token_account(
        &self,
        keypair: &Keypair,
        mint: &Pubkey,
    ) -> OrcaResult<Pubkey> {
        let associated_token_address = self.get_associated_token_address(&keypair.pubkey(), mint);
        match self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .get_account(&associated_token_address)
            .await
        {
            Ok(_) => Ok(associated_token_address),
            Err(_) => self.create_associated_token_account(keypair, mint).await,
        }
    }

    /// Create an associated token account for the given keypair and mint
    ///
    /// # Params
    /// keypair - The keypair that will own the token account
    /// mint - The public key of the token mint
    ///
    /// # Example
    /// ```rust
    /// use solana_sdk::{pubkey, signer::keypair::Keypair};
    ///
    /// # async fn example(client: &OrcaClient) -> OrcaResult<()> {
    /// let keypair = Keypair::new();
    /// let mint = pubkey!("MintPublicKeyHere");
    /// let token_account = client.create_associated_token_account(&keypair, &mint).await?;
    /// println!("Created token account: {}", token_account);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_associated_token_account(
        &self,
        keypair: &Keypair,
        mint: &Pubkey,
    ) -> OrcaResult<Pubkey> {
        let recent_blockhash = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .get_latest_blockhash()
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get blockhash: {}", e)))?;
        let instruction =
            spl_associated_token_account::instruction::create_associated_token_account(
                &keypair.pubkey(),
                &keypair.pubkey(),
                mint,
                &spl_token::id(),
            );
        let message = Message::new(&[instruction], Some(&keypair.pubkey()));
        let transaction = Transaction::new(&[keypair], message, recent_blockhash);
        self.solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .send_and_confirm_transaction(&transaction)
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to create token account: {}", e)))?;
        Ok(self.get_associated_token_address(&keypair.pubkey(), mint))
    }

    /// Get the total supply of a token mint
    ///
    /// # Params
    /// mint - The public key of the token mint
    ///
    /// # Example
    /// ```rust
    /// use solana_sdk::pubkey;
    ///
    /// # async fn example(client: &OrcaClient) -> OrcaResult<()> {
    /// let mint = pubkey!("MintPublicKeyHere");
    /// let supply = client.get_token_supply(&mint).await?;
    /// println!("Token supply: {}", supply);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_token_supply(&self, mint: &Pubkey) -> OrcaResult<u64> {
        let mint_account = self
            .solana
            .client
            .as_ref()
            .ok_or(OrcaError::Error("RPC client not available".to_string()))?
            .get_account(mint)
            .await
            .map_err(|e| OrcaError::Error(format!("Failed to get mint account: {}", e)))?;
        let mint_data: spl_token::state::Mint = spl_token::state::Mint::unpack(&mint_account.data)
            .map_err(|e| OrcaError::Error(format!("Failed to unpack mint data: {}", e)))?;
        Ok(mint_data.supply)
    }

    /// Decode account data from various encoding formats
    ///
    /// # Params
    /// ui_account_data - The UI account data to decode
    ///
    pub fn decode_account_data(&self, ui_account_data: &UiAccountData) -> OrcaResult<Vec<u8>> {
        match ui_account_data {
            UiAccountData::Binary(data, encoding) => match encoding {
                UiAccountEncoding::Base64 => BASE64_STANDARD
                    .decode(data)
                    .map_err(|e| OrcaError::Error(format!("Base64 decode error: {}", e))),
                UiAccountEncoding::Base64Zstd => {
                    let compressed_data = BASE64_STANDARD
                        .decode(data)
                        .map_err(|e| OrcaError::Error(format!("Base64 decode error: {}", e)))?;
                    zstd::decode_all(&compressed_data[..])
                        .map_err(|e| OrcaError::Error(format!("Zstd decode error: {}", e)))
                }
                _ => Err(OrcaError::Error(format!(
                    "Unsupported encoding: {:?}",
                    encoding
                ))),
            },
            _ => Err(OrcaError::Error(
                "Unsupported account data format".to_string(),
            )),
        }
    }
}
