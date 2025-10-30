use solana_network_sdk::Solana;
use solana_sdk::{
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use std::str::FromStr;

use crate::{
    global::{ORCA_STABLE_SWAP_PROGRAM_ID, ORCA_WHIRLPOOLS_PROGRAM_ID},
    types::OrcaError,
};

pub mod balance;
pub mod events;
pub mod global;
pub mod liquidity;
pub mod monitoring;
pub mod pool;
pub mod price;
pub mod trade;
pub mod types;

pub struct OrcaClient {
    pub solana: Solana,
    pub whirlpool_program_id: Pubkey,
    pub stable_swap_program_id: Pubkey,
}

impl OrcaClient {
    pub fn new() -> Result<Self, OrcaError> {
        Ok(Self {
            solana: Solana::new(solana_network_sdk::types::Mode::MAIN)
                .map_err(|e| OrcaError::Error(format!("Failed to create Solana client: {}", e)))?,
            whirlpool_program_id: Pubkey::from_str(ORCA_WHIRLPOOLS_PROGRAM_ID)
                .map_err(|e| OrcaError::Error(format!("Invalid whirlpool program ID: {}", e)))?,
            stable_swap_program_id: Pubkey::from_str(ORCA_STABLE_SWAP_PROGRAM_ID)
                .map_err(|e| OrcaError::Error(format!("Invalid stable swap program ID: {}", e)))?,
        })
    }

    pub fn get_associated_token_address(&self, wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
        spl_associated_token_account::get_associated_token_address(wallet, mint)
    }
}
