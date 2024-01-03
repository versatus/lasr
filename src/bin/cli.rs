#![allow(unused)]
use std::str::FromStr;

use clap::{Parser, Subcommand, ValueEnum, command, Arg, ArgGroup, Command, ArgAction, value_parser, error::{ErrorKind, ContextKind, ContextValue}};

use hex::ToHex;
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use lasr::{account::Address, WalletBuilder, Wallet, PayloadBuilder, LasrRpcClient, Account};
use secp256k1::{SecretKey, Secp256k1, rand::rngs::OsRng, Keypair}; 
use ethereum_types::{Address as EthereumAddress, U256};
use bip39::{Mnemonic, Language};
use std::io::Read;

#[derive(Clone, Debug, ValueEnum)]
enum Unit {
    Echo, 
    Beat,
    Note,
    Chord,
    Harmony,
    Melody,
    Verse,
}

#[derive(Clone, Debug)]
struct U256Wrapper(pub U256);

impl FromStr for U256Wrapper {
    type Err = uint::FromDecStrErr;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(U256Wrapper(
            U256::from_dec_str(s)?
        ))
    }
}

impl std::ops::Mul<&Unit> for U256 {
    type Output = U256;
    fn mul(self, rhs: &Unit) -> Self::Output {
        match rhs {
            Unit::Echo => return self * U256::from(1 as u128),
            Unit::Beat => self * U256::from(1_000 as u128),
            Unit::Note => self * U256::from(1_000_000 as u128),
            Unit::Chord => self * U256::from(1_000_000_000 as u128),
            Unit::Harmony => self * U256::from(1_000_000_000_000 as u128),
            Unit::Melody => self * U256::from(1_000_000_000_000_000 as u128),
            Unit::Verse => self * U256::from(1_000_000_000_000_000_000 as u128)
        }
    }
}

fn parse_u256(s: &str) -> Result<U256, String> {
    U256::from_dec_str(s).map_err(|e| format!("Failed to parse U256: {}", e))
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Creates, signs and sends a `call` transaction to the LASR RPC 
    /// node this wallet interacts with
    Call {
        #[arg(short, long, required=false)]
        wallet: u8,
        #[arg(short, long, required=false)]
        to: String, 
        #[arg(long, required=true)]
        program_id: String,
        #[arg(short, long, required=true)]
        op: String,
        #[arg(short, long, required=true)]
        inputs: String,
        #[arg(short, long, required=true)]
        value: u128,
        #[arg(short, long, required=false)]
        unit: Unit,
    },
    /// Creates, signs and sends a `deploy` transaction to the LASR RPC
    /// node this wallet interacts with
    Deploy {
        #[arg(short, long, required=false)]
        wallet: u8,
       #[arg(short, long, required=true)] 
        content_id: String
    },
    BridgeOut,
    GetBalance {
        address: String,
        program_id: String,
    },
    GetNonce,
}

fn new_wallet_command() -> Command {
    Command::new("new")
        .about("creates a new keypair, provides the mnemonic, secret key, public key, and address to the user")
        .propagate_version(true)
}

fn from_mnemonic_command() -> Command {
    Command::new("mnemonic")
        .about("creates/loads a wallet keypair from a mnemonic")
        .arg_required_else_help(true)
        .arg(
            Arg::new("mnemonic")
                .short('m')
                .long("mnemonic")
                .help("a series of either 12 or 24 words")
                .action(
                    ArgAction::Append
                )
                .required(true)
        )
}

fn from_secret_key_command() -> Command {
    Command::new("secret-key")
        .about("creates/loads a wallet keypair from a secret key, provides the secret key, public key and address to the user")
        .arg_required_else_help(true)
        .arg(
            Arg::new("secret-key")
                .short('s')
                .long("secret-key")
                .aliases(["sk", "secretkey", "private-key", "privatekey", "pk", "secret_key", "private_key"])
                .help("A hexadecimal string representing a secp256k1 ECDSA secret key")
        )
}

fn from_file_arg() -> Arg {
    Arg::new("from-file")
        .long("from-file")
        .aliases(["ff", "from_file", "fromfile", "load"])
        .action(ArgAction::SetTrue)
        .requires("path")
        .required(false)
        .conflicts_with("from-mnemonic")
        .conflicts_with("from-secret-key")
}

fn keyfile_path_arg() -> Arg {
    Arg::new("path")
        .short('p')
        .long("path")
        .help("The path to the keypair file used to load the wallet")
        .default_value("../../keypair.json")
}

fn wallet_index_arg() -> Arg {
    Arg::new("wallet-index")
        .short('w')
        .long("wallet-index")
        .aliases(["index", "wallet", "walletindex", "wi", "i"])
        .help("The index in the array in the keypair file of the wallet that the user would like to use")
        .value_parser(value_parser!(u8))
        .default_missing_value("0")
}

fn from_file_group() -> ArgGroup {
    ArgGroup::new("from-file-args")
        .required(false)
        .args(["from-file", "path", "wallet-index"])
}

fn from_mnemonic_arg() -> Arg {
    Arg::new("from-mnemonic")
        .long("from-mnemonic")
        .aliases(["fm", "from_mnemonic", "from_m", "from-m"])
        .help("A flag stating that a mnemonic phrase consisting of 12 or 24 words to derive a keypair from will be used for the wallet")
        .action(ArgAction::SetTrue)
        .requires("mnemonic")
        .conflicts_with("from-file")
        .conflicts_with("from-secret-key")
}

fn mnemonic_arg() -> Arg {
    Arg::new("mnemonic")
        .short('m')
        .long("mnemonic")
        .aliases(["words", "phrase", "passphrase"])
        .help("A mnemonic phrase consisting of 12 or 24 words that will be used to derive a keypair for the wallet from")
        .action(ArgAction::Append)
        .requires("from-mnemonic")
}

fn from_mnemonic_group() -> ArgGroup {
    ArgGroup::new("from-mnemonic-args")
        .required(false)
        .args(["from-mnemonic", "mnemonic"])
}

fn from_secret_key_arg() -> Arg {
    Arg::new("from-secret-key")
        .long("from-secret-key")
        .aliases(["fsk", "fromsecretkey", "from-sk", "fromsk", "frompk", "fpk", "from-private-key", "fromprivatekey"])
        .help("A flag stating that a secret key will be used to build a wallet")
        .action(ArgAction::SetTrue)
        .requires("secret-key")
        .conflicts_with("from-file")
        .conflicts_with("from-mnemonic")
}

fn secret_key_arg() -> Arg {
    Arg::new("secret-key")
        .short('s')
        .long("secret-key")
        .aliases(["sk", "pk", "secretkey", "secret_key", "private-key", "privatekey", "private_key"])
        .help("A hexidecimal string representing a secret key to be used to build a wallet")
}

fn from_secret_key_group() -> ArgGroup {
    ArgGroup::new("from-secret-key-args")
        .required(false)
        .args(["from-secret-key", "secret-key"])
}

fn receiver_arg() -> Arg {
    Arg::new("to")
        .short('t')
        .long("to")
        .alias("receiver")
        .help("The receiver of the tokens being sent, should be a 40 character hexidecimal string, representing a 20 byte address")
        .required(true)
        .value_name("Address")
        .value_parser(value_parser!(Address))
}

fn program_id_arg() -> Arg {
    Arg::new("program-id")
        .short('i')
        .long("program-id")
        .aliases(["pid", "prog-id", "program_id", "prog_id", "id", "token", "token-id", "token_id", "tid"])
        .help("A hexidecimal string representing the program id of the token that is being sent, defaults to 0 for native ETH tokens and 1 for native Verse tokens")
        .default_value("0")
        .default_missing_value("0")
        .value_name("Address")
        .value_parser(value_parser!(Address))
}

fn value_arg() -> Arg {
    Arg::new("value")
        .short('v')
        .long("value")
        .aliases(["amount", "a", "amt", "nft-id"])
        .required(true)
        .value_name("U256")
        .value_parser(value_parser!(U256Wrapper))
        .help("the amount of the token to send")
}

fn unit_arg() -> Arg {
    Arg::new("unit")
        .short('u')
        .long("unit")
        .default_value("echo")
        .value_parser(value_parser!(Unit))
        .required(true)
}

fn send_command() -> Command {
    Command::new("send")
        .about("creates, signs and sends a simple token transfer transaction to the RPC server the wallet is configured to interact with")
        .arg(from_file_arg())
        .arg(keyfile_path_arg())
        .arg(wallet_index_arg())
        .arg(from_mnemonic_arg())
        .arg(mnemonic_arg())
        .arg(from_secret_key_arg())
        .arg(secret_key_arg())
        .arg(
            receiver_arg()
        )
        .arg(
            program_id_arg()
        )
        .arg(
            value_arg()
        )
        .arg(
            unit_arg()
        )
}

fn wallet_command() -> Command {
    Command::new("wallet")
        .about("a wallet cli for interacting with the LASR network as a user")
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            new_wallet_command()
        )
        .subcommand(
            from_mnemonic_command()
        )
        .subcommand(
            from_secret_key_command()
        )
        .subcommand(
            send_command()
        )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let matches = command!()
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .about("A cli interacting with the LASR network")
        .subcommand(
            wallet_command()
        )
        .get_matches();

    match matches.subcommand() {
        Some(("wallet", sub)) => {
            match sub.subcommand() {
                Some(("new", children)) => {
                    println!("received `wallet new` command");
                }
                Some(("mnemonic", children)) => {
                    println!("received `wallet mnemonic` command");
                }
                Some(("secret-key", children)) => {
                    println!("received `wallet secret-key` command");
                },
                Some(("send", children)) => {
                    println!("received `wallet send` command");
                    let values = children.ids();

                    let from_file = children.get_flag("from-file");
                    let from_mnemonic = children.get_flag("from-mnemonic");
                    let from_secret_key = children.get_flag("from-secret-key");
                    
                    let mut wallet = {
                        let (secret_key, public_key) = {
                            if from_file {
                                let keypair_file = children.get_one::<String>("path").expect("required if from-file flag");
                                dbg!(keypair_file);
                                let mut file = std::fs::File::open(keypair_file)?;
                                let mut contents = String::new();
                                file.read_to_string(&mut contents);
                                let keypair: Keypair = serde_json::from_str(&contents)?;
                                let master = keypair.secret_key();
                                let pubkey = keypair.public_key();
                                (master, pubkey)
                            } else if from_mnemonic {
                                let phrase = children.get_one::<String>("mnemonic").expect("required if from-mnemonic flag");
                                let mnemonic = Mnemonic::parse_in_normalized(Language::English, &phrase)?;  
                                let seed = mnemonic.to_seed("");
                                let secp = Secp256k1::new();
                                let master = SecretKey::from_slice(&seed[0..32])?;
                                let pubkey = master.public_key(&secp);
                                (master, pubkey)
                            } else {
                                let sk = children.get_one::<String>("secret-key").expect("required if from-secret-key flag");
                                let secp = Secp256k1::new();
                                let master = SecretKey::from_str(&sk)?; 
                                let pubkey = master.public_key(&secp);
                                let address: Address = pubkey.into();
                                let eaddr: EthereumAddress = address.into();
                                println!("************************************************************");
                                println!("****************  DO NOT SHARE SECRET KEY  *****************");
                                println!("************************************************************\n");
                                println!("************************************************************");
                                println!("******************     SECRET KEY        *******************\n");
                                println!("{}\n", &sk);
                                println!("************************************************************\n");
                                println!("************************************************************");
                                println!("******************       ADDRESS         *******************");
                                println!("*******  0x{}  *******", &address.encode_hex::<String>());
                                println!("************************************************************\n");
                                (master, pubkey)
                            } 
                        };

                        let address: Address = public_key.into();
                        let client = HttpClientBuilder::default().build("http://127.0.0.1:9292")?;
                        let account: Account = bincode::deserialize(
                            &client.get_account(format!("{:x}", address)).await?
                        )?;

                        WalletBuilder::default()
                            .sk(secret_key)
                            .client(client.clone())
                            .address(address.clone())
                            .builder(PayloadBuilder::default())
                            .account(account)
                            .build()
                    }?;

                    let address = wallet.address();

                    wallet.get_account(&address).await;

                    let to = children.get_one::<Address>("to").expect("required");
                    let pid = children.get_one::<Address>("program-id").expect("required");
                    let value = children.get_one::<U256Wrapper>("value").expect("required");
                    let unit = children.get_one::<Unit>("unit").expect("required");

                    let amount = value.0 * unit; 

                    let token = wallet.send(to, pid, amount).await;


                }
                _ => {}

            }
        }
        _ => {}

    }

    Ok(())
}
