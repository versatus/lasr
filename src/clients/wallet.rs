#![allow(unused)]
use bip39::{Mnemonic, Language};
use rand::{RngCore, SeedableRng, rngs::StdRng};
use ethereum_types::U256;
use secp256k1::{SecretKey, Secp256k1, Message, Keypair, hashes::{sha256, Hash}, PublicKey};
use serde::{Deserialize, Serialize, Serializer, Deserializer};
use sha3::{Digest, Sha3_256};
use crate::{
    LasrRpcClient,
    PayloadBuilder, 
    Address, 
    TransactionType, 
    Account, 
    RecoverableSignature, 
    Transaction, Token,
};

pub type WalletError = Box<dyn std::error::Error + Send>;
pub type WalletResult<T> = Result<T, WalletError>;

#[derive(Builder, Clone, Serialize, Deserialize)]
pub struct WalletInfo {
    mnemonic: Mnemonic,
    keypair: Keypair,
    secret_key: SecretKey,
    public_key: PublicKey,
    address: Address
}

impl WalletInfo {
    pub fn mnemonic(&self) -> &Mnemonic {
        &self.mnemonic
    }

    pub fn address(&self) -> &Address {
        &self.address
    }

    pub fn secret_key(&self) -> SecretKey {
        self.secret_key
    }

    pub fn public_key(&self) -> PublicKey {
        self.public_key
    }
}

#[derive(Builder, Clone)]
pub struct Wallet<L> 
where 
    L: LasrRpcClient
{
    client: L,
    sk: SecretKey,
    builder: PayloadBuilder,
    address: Address,
    account: Account
}

impl<L: LasrRpcClient + Send + Sync> Wallet<L> {

    pub fn new(
        seed: Option<&u128>,
        passphrase: Option<&String>,
        size: Option<&usize>
    ) -> WalletResult<WalletInfo> {
        let unwrapped_seed = if let Some(s) = seed {
            s.clone()
        } else {
           let mut rng = StdRng::from_entropy();
           let seed = rng.next_u64();
           seed as u128
        };

        let unwrapped_passphrase = if let Some(p) = passphrase {
           p.clone()
        } else {
           "".to_string()
        };

        let unwrapped_size = if let Some(s) = size {
            *s
        } else {
            12usize
        };

        let mut rng = StdRng::from_entropy();
        let mnemonic = if unwrapped_size == 12 {
            let mut entropy_12 = [0u8; 16];
            rng.fill_bytes(&mut entropy_12);
            let mnemonic = Mnemonic::from_entropy(&entropy_12).map_err(|e| {
               Box::new(e) as Box<dyn std::error::Error + Send>
            })?;
            mnemonic
        } else {
            let mut entropy_24 = [0u8; 32];
            rng.fill_bytes(&mut entropy_24);
            let mnemonic = Mnemonic::from_entropy(&entropy_24).map_err(|e| {
               Box::new(e) as Box<dyn std::error::Error + Send>
            })?;
            mnemonic
        };

        let keypair_seed = mnemonic.to_seed(unwrapped_passphrase);

        let secp = Secp256k1::new();
        let mut hasher = Sha3_256::new();
        hasher.update(keypair_seed);
        let seed_hash = hasher.finalize().to_vec();
        let secret_key = SecretKey::from_hashed_data::<sha256::Hash>(&seed_hash[..]); 
        let keypair = Keypair::from_secret_key(&secp, &secret_key);
        let public_key = keypair.public_key();
        let address = Address::from(keypair.public_key());

        Ok(WalletInfo {
            mnemonic,
            keypair,
            secret_key,
            public_key,
            address
        })
    }
    
    pub async fn send(
        &mut self,
        to: &Address,
        program_id: &Address,
        value: crate::U256,
    ) -> WalletResult<Token> {
        let account = self.account();
        let address = self.address();

        account.validate_balance(program_id, value)?;

        let tx_nonce = {
            crate::U256::from(account.nonce() + ethereum_types::U256::from(1).into())
        };
        let payload = self.builder
            .transaction_type(TransactionType::Send(account.nonce()))
            .from(address.into())
            .to(to.into())
            .program_id(program_id.into())
            .inputs(String::new())
            .op(String::new())
            .value(value)
            .nonce(tx_nonce)
            .build().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let msg = Message::from_digest_slice(&payload.hash()).map_err(|e| {
            Box::new(e) as Box<dyn std::error::Error + Send>
        })?;

        let context = Secp256k1::new();

        let sig: RecoverableSignature = context.sign_ecdsa_recoverable(&msg, &self.sk).into();

        let transaction: Transaction = (payload, sig.clone()).into();

        let token: Token = bincode::deserialize(
            &self.client.send(
                transaction.clone()
            ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?
        ).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.get_account(&self.address()).await?;
        self.increment_nonce();

        Ok(token)
    }

    pub async fn call(
        &mut self,
        program_id: &Address,
        to: &Address,
        value: crate::U256,
        op: &String,
        inputs: &String,
    ) -> WalletResult<Account> {

        let account = self.account();
        let address = self.address();

        dbg!("validating balance");

        if value > U256::from(0).into() {
            account.validate_balance(&program_id, value)?;
        }

        dbg!("building transaciton payload");
        let payload = self.builder 
            .transaction_type(TransactionType::Call(account.nonce()))
            .from(address.into())
            .to(to.into())
            .program_id(program_id.into())
            .inputs(inputs.to_string())
            .op(op.to_string())
            .value(value)
            .nonce(account.nonce())
            .build().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let msg = Message::from_digest_slice(&payload.hash()).map_err(|e| {
            Box::new(e) as Box<dyn std::error::Error + Send>
        })?;

        let context = Secp256k1::new();

        dbg!("signing transaaction");
        let sig: RecoverableSignature = context.sign_ecdsa_recoverable(&msg, &self.sk).into();

        dbg!("packaging transaaction");
        let transaction: Transaction = (payload, sig.clone()).into();

        dbg!("submitting transaction to RPC");
        let account_str = self.client.call(
            transaction.clone()
        ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let account = serde_json::from_str(&account_str).map_err(|e| {
            Box::new(e) as Box<dyn std::error::Error + Send>
        })?;

        self.get_account(&self.address()).await?;

        Ok(account)
    }

    pub async fn register_program(&mut self, inputs: &String) -> WalletResult<()> {
        let account = self.account();
        let address = self.address();

        let payload = self.builder 
            .transaction_type(TransactionType::RegisterProgram(account.nonce()))
            .from(address.into())
            .to([0; 20])
            .program_id([0; 20])
            .inputs(inputs.to_string())
            .op(String::from(""))
            .value(U256::from(0).into())
            .nonce(account.nonce())
            .build().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let msg = Message::from_digest_slice(&payload.hash()).map_err(|e| {
            Box::new(e) as Box<dyn std::error::Error + Send>
        })?;

        let context = Secp256k1::new();

        let sig: RecoverableSignature = context.sign_ecdsa_recoverable(&msg, &self.sk).into();

        let transaction: Transaction = (payload, sig.clone()).into();

        //TODO: return `payment token` with approval set to Address(0), i.e. network
        //should be able to pull fees from the contract deployer/owner account
        let _ = self.client.register_program(
            transaction.clone()
        ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.get_account(&self.address()).await?;

        Ok(())
    }

    pub async fn get_account(&mut self, address: &Address) -> WalletResult<()> {
        log::info!("calling get_account for {:x}", address);
        let account: Account = serde_json::from_str(
            &self.client.get_account(format!("{:x}", address)).await.map_err(|e| {
                Box::new(e) as Box<dyn std::error::Error + Send>
            })?
        ).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.account = account;

        println!("\n");
        println!("****************** Wallet Balances ********************");
        println!("*       Token           |           Balance           *");
        println!("* ----------------------|---------------------------- *");
        for (id, token) in self.account().programs() {
            println!("*    {:<23}      | {:>23}     *", id, token.balance());
        }
        println!("*******************************************************");
        println!("\n");

        Ok(())
    }

    pub fn address(&self) -> Address {
        self.address
    }

    fn account_mut(&mut self) -> &mut Account {
        &mut self.account
    }

    fn increment_nonce(&mut self) {
        self.account_mut().increment_nonce();
    }

    pub(crate) fn account(&self) -> Account {
        self.account.clone()
    }
}
