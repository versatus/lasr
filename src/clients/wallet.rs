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
    Transaction, Token
};

pub type WalletError = Box<dyn std::error::Error + Send>;
pub type WalletResult<T> = Result<T, WalletError>;

fn serialize_as_hex<S>(bytes: impl Into<[u8; 20]>, serializer: S) -> Result<S::Ok, S::Error> 
where
    S: Serializer
{
    let hex = bytes.into().iter().map(|b| format!("{:02x}", b)).collect::<String>();
    let hex_repr = format!("0x{}", hex);
    serializer.serialize_str(&hex_repr)
}

fn deserialize_from_hex<'de, D>(deserializer: D) -> Result<Address, D::Error> 
where 
    D: Deserializer<'de>
{
    let s = String::deserialize(deserializer)?;
    if s.len() != 42 {
        return Err(serde::de::Error::custom("Invalid length"))
    }

    if !s.starts_with("0x") {
        return Err(serde::de::Error::custom("'0x' prefix missing"))
    }

    let bytes = hex::decode(&s[2..]).map_err(serde::de::Error::custom)?;
    let mut arr = [0u8; 20];
    arr.copy_from_slice(&bytes[0..20]);
    Ok(Address::from(arr))
}

#[derive(Builder, Clone, Serialize, Deserialize)]
pub struct WalletInfo {
    mnemonic: Mnemonic,
    keypair: Keypair,
    secret_key: SecretKey,
    public_key: PublicKey,
    #[serde(
        serialize_with = "serialize_as_hex",
        deserialize_with = "deserialize_from_hex"
    )]
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
        value: U256,
    ) -> WalletResult<Token> {
        let account = self.account();
        let address = self.address();

        account.validate_balance(program_id, value)?;

        let payload = self.builder
            .transaction_type(TransactionType::Send(account.nonce()))
            .from(address.into())
            .to(to.into())
            .program_id(program_id.into())
            .inputs(String::new())
            .op(String::new())
            .value(value)
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

        Ok(token)
    }

    pub async fn call(
        &mut self,
        program_id: &Address,
        to: &Address,
        value: U256,
        op: &String,
        inputs: &String,
    ) -> WalletResult<Vec<Token>> {

        let account = self.account();
        let address = self.address();

        account.validate_balance(&program_id, value)?;

        let payload = self.builder 
            .transaction_type(TransactionType::Send(account.nonce()))
            .from(address.into())
            .to(to.into())
            .program_id(program_id.into())
            .inputs(inputs.to_string())
            .op(op.to_string())
            .value(value)
            .build().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let msg = Message::from_digest_slice(&payload.hash()).map_err(|e| {
            Box::new(e) as Box<dyn std::error::Error + Send>
        })?;

        let context = Secp256k1::new();

        let sig: RecoverableSignature = context.sign_ecdsa_recoverable(&msg, &self.sk).into();

        let transaction: Transaction = (payload, sig.clone()).into();

        let tokens = self.client.call(
            transaction.clone()
        ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        self.get_account(&self.address()).await?;

        Ok(tokens)
    }

    pub async fn register_program(&mut self, inputs: &String) -> WalletResult<()> {
        let account = self.account();
        let address = self.address();

        let payload = self.builder 
            .transaction_type(TransactionType::Deploy(account.nonce()))
            .from(address.into())
            .to([0; 20])
            .program_id([0; 20])
            .inputs(inputs.to_string())
            .op(String::from("register"))
            .value(0.into())
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
        let account: Account = bincode::deserialize(
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

    pub(crate) fn account(&self) -> Account {
        self.account.clone()
    }
}
