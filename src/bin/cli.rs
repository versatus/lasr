#![allow(unused)]
use std::fs::OpenOptions;
use std::{str::FromStr, path::Path};
use std::io::Write;
use clap::{Parser, Subcommand, ValueEnum, command, Arg, ArgGroup, Command, ArgAction, value_parser, error::{ErrorKind, ContextKind, ContextValue}, ArgMatches};

use hex::ToHex;
use jsonrpsee::{http_client::{HttpClient, HttpClientBuilder}, core::client::ClientT};
use lasr::Transaction;
use lasr::{account::Address, WalletBuilder, Wallet, PayloadBuilder, LasrRpcClient, Account, WalletInfo, Namespace};
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
struct U256Wrapper(pub lasr::U256);

impl FromStr for U256Wrapper {
    type Err = uint::FromDecStrErr;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(U256Wrapper(
            ethereum_types::U256::from_dec_str(s)?.into()
        ))
    }
}

impl std::ops::Mul<&Unit> for lasr::U256 {
    type Output = lasr::U256;
    fn mul(self, rhs: &Unit) -> Self::Output {
        match rhs {
            Unit::Echo => return (U256::from(self) * U256::from(1 as u128)).into(),
            Unit::Beat => return (U256::from(self) * U256::from(1_000 as u128)).into(),
            Unit::Note => return (U256::from(self) * U256::from(1_000_000 as u128)).into(),
            Unit::Chord => return (U256::from(self) * U256::from(1_000_000_000 as u128)).into(),
            Unit::Harmony => return (U256::from(self) * U256::from(1_000_000_000_000 as u128)).into(),
            Unit::Melody => return (U256::from(self) * U256::from(1_000_000_000_000_000 as u128)).into(),
            Unit::Verse => return (U256::from(self) * U256::from(1_000_000_000_000_000_000 as u128)).into()
        }
    }
}

fn parse_u256(s: &str) -> Result<lasr::U256, String> {
    Ok(ethereum_types::U256::from_dec_str(s).map_err(|e| format!("Failed to parse U256: {}", e))?.into())
}

#[derive(Debug, Subcommand)]
enum Commands {
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
        .arg(
            seed_arg()
                .required(false)
        )
        .arg(
            passphrase_arg()
                .required(false)
        )
        .arg(
            mnemonic_size_arg()
                .required(false)
        )
        .arg(
            keyfile_path_arg()
                .required(false)
        )
        .arg(
            save_to_file_arg()
                .required(false)
        )
        .arg(
            overwrite_path_arg()
                .required(false)
        )
}

fn from_mnemonic_command() -> Command {
    Command::new("mnemonic")
        .about("creates/loads a wallet keypair from a mnemonic")
        .arg_required_else_help(true)
        .arg(
            mnemonic_arg()
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
        .required(false)
        .conflicts_with("from-mnemonic")
        .conflicts_with("from-secret-key")
}

fn keyfile_path_arg() -> Arg {
    Arg::new("path")
        .long("path")
        .help("The path to the keypair file used to load the wallet")
        .default_value(".lasr/wallet/keypair.json")
}

fn overwrite_path_arg() -> Arg {
    Arg::new("overwrite")
        .long("overwrite")
        .aliases(["replace"])
        .default_value("false")
        .value_parser(value_parser!(bool))
        .action(ArgAction::SetTrue)
}

fn wallet_index_arg() -> Arg {
    Arg::new("wallet-index")
        .short('w')
        .long("wallet-index")
        .aliases(["index", "wallet", "walletindex", "wi", "i"])
        .help("The index in the array in the keypair file of the wallet that the user would like to use")
        .value_parser(value_parser!(usize))
        .required(false)
        .default_value("0")
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

fn mnemonic_size_arg() -> Arg {
    Arg::new("mnemonic-size")
        .short('n')
        .long("size")
        .value_parser(value_parser!(usize))
        .help("The size of the mnemonic phrase, can be 12 or 24, defaults to 12")
}

fn seed_arg() -> Arg {
    Arg::new("seed")
        .short('s')
        .long("seed")
        .value_parser(value_parser!(u128))
        .help("A 128-bit number to generate an entropy from. WARNING: only provide this if you know what you are doing")
}

fn passphrase_arg() -> Arg {
    Arg::new("passphrase")
        .short('p')
        .long("passphrase")
        .help("A user selected passphrase that is used to generate a random seed")
}

fn save_to_file_arg() -> Arg {
    Arg::new("save")
        .short('o')
        .long("save")
        .value_parser(value_parser!(bool))
        .action(ArgAction::SetTrue)
        .help("Save the wallet info to a file")
        .default_value("false")
}

fn receiver_arg() -> Arg {
    Arg::new("to")
        .short('t')
        .long("to")
        .alias("receiver")
        .help("The receiver of the tokens being sent, should be a 40 character hexidecimal string, representing a 20 byte address")
        .value_name("Address")
        .value_parser(value_parser!(Address))
        .required(true)
}

fn program_id_arg() -> Arg {
    Arg::new("content-namespace")
        .short('c')
        .long("content-namespace")
        .aliases(["pid", "prog-id", "program_id", "prog_id", "id", "token", "token-id", "token_id", "tid", "content-id", "cid", "content-namespace", "cns"])
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
        .default_value("0")
        .help("the amount of the token to send")
}

fn unit_arg() -> Arg {
    Arg::new("unit")
        .short('u')
        .long("unit")
        .default_value("echo")
        .value_parser(value_parser!(Unit))
        .required(false)
        .help("The unit of tokens to be applied to the value")
}

fn function_arg() -> Arg {
    Arg::new("op")
        .long("op")
        .aliases(["function", "fn", "func", "def", "method", "call"])
        .required(true)
        .help("the operation or function in the contract being called that will be executed")
}

fn inputs_arg() -> Arg {
    Arg::new("inputs")
        .short('i')
        .long("inputs")
        .aliases(["params", "function-inputs", "op-inputs", "op-params"])
        .required(true)
        .help("a json string that represents the inputs to the function, i.e. function parameters/arguments for the function in the contract that will be called")
}

fn register_program_command() -> Command {
    Command::new("register-program")
        .aliases(["deploy", "rp", "register", "reg-prog", "prog", "deploy-prog", "register-contract", "deploy-contract", "rc", "dc"])
        .arg(from_file_arg())
        .arg(keyfile_path_arg())
        .arg(wallet_index_arg())
        .arg(from_mnemonic_arg())
        .arg(mnemonic_arg())
        .arg(from_secret_key_arg())
        .arg(secret_key_arg())
        .arg(inputs_arg())
}

fn call_command() -> Command {
    Command::new("call")
        .about("creates, signs and sends a contract or compute transaction to the RPC server the wallet is configured to interact with")
        .arg(from_file_arg())
        .arg(keyfile_path_arg().required(false))
        .arg(wallet_index_arg())
        .arg(from_mnemonic_arg())
        .arg(mnemonic_arg())
        .arg(from_secret_key_arg())
        .arg(secret_key_arg())
        .arg(receiver_arg())
        .arg(program_id_arg())
        .arg(function_arg())
        .arg(inputs_arg())
        .arg(value_arg().required(false))
        .arg(unit_arg().required(false))
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
        .arg(receiver_arg())
        .arg(program_id_arg())
        .arg(value_arg())
        .arg(unit_arg())
}

fn get_account_command() -> Command {
    Command::new("get-account")
        .about("gets the account given a secret key or mnemonic")
        .arg(from_file_arg())
        .arg(keyfile_path_arg())
        .arg(wallet_index_arg())
        .arg(from_mnemonic_arg())
        .arg(mnemonic_arg())
        .arg(from_secret_key_arg())
        .arg(secret_key_arg())
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
        .subcommand(
            call_command()
        )
        .subcommand(
            register_program_command()
        )
        .subcommand(
            get_account_command()
        )
}

fn from_json_file_arg() -> Arg {
    Arg::new("from-file")
        .short('f')
        .long("from-file")
        .value_parser(value_parser!(bool))
        .action(ArgAction::SetTrue)
        .default_value("false")
}

fn json_arg() -> Arg {
    Arg::new("json")
        .short('j')
        .long("json")
}

fn parse_outputs() -> Command {
    Command::new("parse-outputs")
        .arg(from_json_file_arg())
        .arg(json_arg())
}

fn parse_transaction() -> Command {
    Command::new("parse-transaction")
        .aliases(["parse-tx", "parse-tx-json"])
        .arg(json_arg())
}

fn get_default_transaction() -> Command {
    Command::new("default-transaction")
        .aliases(["default-tx", "tx"])
}

fn hex_or_bytes() -> Command {
    Command::new("hex-or-bytes")
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
        .subcommand(
            parse_outputs()
        )
        .subcommand(
            parse_transaction()
        )
        .subcommand(
            get_default_transaction()
        )
        .subcommand(
            hex_or_bytes()
        )
        .get_matches();

    match matches.subcommand() {
        Some(("wallet", sub)) => {
            match sub.subcommand() {
                Some(("new", children)) => {
                    let values = children.ids();
                    let seed = children.get_one::<u128>("seed");
                    let passphrase = children.get_one::<String>("passphrase");
                    let size = children.get_one::<usize>("mnemonic-size");
                    let wallet_info = Wallet::<HttpClient>::new(seed, passphrase, size)
                        .expect("Unable to acquire WalletInfo");
                    pretty_print_wallet_info(&wallet_info);

                    let save = children.get_one::<bool>("save");
                    if let Some(true) = save {
                        let path = std::path::Path::new(children.get_one::<String>("path").expect("required or default"));
                        let dir_path = path.parent().expect("dir has no parent");
                        if !dir_path.exists() {
                            std::fs::create_dir_all(dir_path).expect("unable to create directory for keyfile");
                        }
                        let overwrite = children.get_one::<bool>("overwrite").expect("required or default");
                        let mut file_buffer = Vec::new();
                        let mut existing = if path.exists() {
                            std::fs::File::open(path).expect("file cant be opened")
                                .read_to_end(&mut file_buffer).expect("file should exist");
                            let mut inner = if &file_buffer.len() > &0 {
                                serde_json::from_slice::<Vec<WalletInfo>>(&file_buffer)
                                    .expect("keypair file has been corrupted")
                            } else {
                                Vec::new()
                            };
                                inner
                        } else {
                            Vec::new()
                        };
                        
                        let mut file = std::fs::OpenOptions::new()
                            .write(true)
                            .read(true)
                            .create(true)
                            .truncate(true)
                            .open(path)
                            .expect("unable to open file to write WalletInfo");

                        existing.push(wallet_info);
                        file.write_all(
                            serde_json::to_string_pretty(&existing)
                                .expect("unable to serialize wallet info")
                                .as_bytes()
                        ).expect("unable to write wallet info to file");
                    }
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
                    let mut wallet = get_wallet(children).await?;

                    let address = wallet.address();

                    wallet.get_account(&address).await;

                    let to = children.get_one::<Address>("to").expect("required");
                    let pid = children.get_one::<Address>("content-namespace").expect("required");
                    let value = children.get_one::<U256Wrapper>("value").expect("required");
                    let unit = children.get_one::<Unit>("unit").expect("required");

                    let amount = value.0 * unit; 

                    let token = wallet.send(to, pid, amount).await;
                }
                Some(("call", children)) => {
                    let values = children.ids();
                    let mut wallet = get_wallet(children).await?;
                    let address = wallet.address();
                    dbg!("attempting to get account");
                    wallet.get_account(&address).await;

                    dbg!("parsing cli args");
                    let to = children.get_one::<Address>("to").expect("required");
                    let pid = children.get_one::<Address>("content-namespace").expect("required");
                    let value = children.get_one::<U256Wrapper>("value").expect("required");
                    let op = children.get_one::<String>("op").expect("required");
                    let inputs = children.get_one::<String>("inputs").expect("required");
                    let unit = children.get_one::<Unit>("unit").expect("required");

                    let amount = value.0 * unit;

                    dbg!("submitting transaction");
                    let tokens = wallet.call(pid, to, amount, op, inputs).await;
                    dbg!("call result: {:?}", tokens);

                }
                Some(("register-program", children)) => {
                    dbg!("Getting wallet");
                    let mut wallet = get_wallet(children).await?;
                    // dbg!("Wallet obtained: {:?}", wallet);
                    let address = wallet.address();
                    println!("Wallet address: {:?}", address);
                    dbg!("Getting account");
                    wallet.get_account(&address).await;
                    dbg!("Account obtained");
                    dbg!("Getting inputs");
                    let inputs = children.get_one::<String>("inputs").expect("required");
                    dbg!("Inputs obtained: {:?}", inputs);
                    dbg!("Registering program");
                    let _ = wallet.register_program(inputs).await;
                    dbg!("Program registered");
                    dbg!("Getting account again");
                    wallet.get_account(&address).await;
                    dbg!("Account obtained again");

                }
                Some(("get-account", children)) => {
                    let mut wallet = get_wallet(children).await?;
                    let address = wallet.address();
                    wallet.get_account(&address).await;
                }
                _ => {}

            }
        }

        Some(("parse-outputs", children)) => {
            let values = children.ids();
            let from_file = children.get_one::<bool>("from-file");
            if let Some(true) = from_file {
                let json = children.get_one::<String>("json").expect("unable to acquire json filepath");
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(false)
                    .append(false)
                    .truncate(false)
                    .create_new(false)
                    .open(json).expect("unable to open json file");

                let mut json_str = String::new();
                file.read_to_string(&mut json_str).expect("unable to read json file contents to string");
                let outputs: lasr::Outputs = serde_json::from_str(&json_str).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
                println!("{:#?}", outputs);
            } else {
                let json_str = children.get_one::<String>("json").expect("unable to acquire json from command line");
                let outputs: lasr::Outputs = serde_json::from_str(&json_str).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
                println!("{:#?}", outputs);
            }
        }
        Some(("parse-transaction", children)) => {
            let value = children.ids();
            let json_str = children.get_one::<String>("json").expect("unable to acquire json from command line");
            let transaction: lasr::Transaction = serde_json::from_str(&json_str).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
            println!("{:#?}", transaction);
        }
        Some(("default-transaction", children)) => {
            println!("{:?}", serde_json::to_string(&Transaction::default()).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?);
        }
        Some(("hex-or-bytes", children)) => {
            let hex = lasr::HexOr20Bytes::Hex(hex::encode(&[2; 20]));
            let bytes = lasr::HexOr20Bytes::Bytes([2; 20]);
            println!("{:?}", serde_json::to_string(&hex).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?);
            println!("{:?}", serde_json::to_string(&bytes).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?);

        }
        _ => {}

    }

    Ok(())
}

async fn get_wallet(children: &ArgMatches) -> Result<Wallet<HttpClient>, Box<dyn std::error::Error>> {
    let values = children.ids();

    let from_file = children.get_flag("from-file");
    let from_mnemonic = children.get_flag("from-mnemonic");
    let from_secret_key = children.get_flag("from-secret-key");
    
    let mut wallet = {
        let (secret_key, public_key) = {
            if from_file {
                let keypair_file = children.get_one::<String>("path").expect("required if from-file flag");
                let wallet_index = children.get_one::<usize>("wallet-index").expect("required or default");
                let mut file = std::fs::File::open(keypair_file)?;
                let mut contents = String::new();
                file.read_to_string(&mut contents);
                let keypair_file: Vec<WalletInfo> = serde_json::from_str(&contents)?;
                let wallet_info = &keypair_file[*wallet_index];
                let master = wallet_info.secret_key();
                let pubkey = wallet_info.public_key();
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
        let lasr_rpc_url = std::env::var("LASR_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:9292".to_string());
        let client = HttpClientBuilder::default().build(lasr_rpc_url)?;
        let res = &client.get_account(format!("{:x}", address)).await;
        let account = if let Ok(account_str) = res {
            println!("{}", account_str);
            let account: Account = serde_json::from_str(&account_str)?;
            account
        } else {
            Account::new(lasr::AccountType::User, None, address, None)
        };

        WalletBuilder::default()
            .sk(secret_key)
            .client(client.clone())
            .address(address.clone())
            .builder(PayloadBuilder::default())
            .account(account)
            .build().map_err(|e| {
                Box::new(e) as Box<dyn std::error::Error>
            })
    };

    wallet
}

fn pretty_print_wallet_info(wallet_info: &WalletInfo) {
    println!("************************************************************");
    println!("****************  DO NOT SHARE MNEMONIC PHRASE *************");
    println!("************************************************************");
    println!("************************************************************");
    println!("******************     MNEMONIC PHRASE     *****************\n");
    println!("{}\n", &wallet_info.mnemonic());
    println!("****************  DO NOT SHARE SECRET KEY  *****************");
    println!("************************************************************\n");
    println!("************************************************************");
    println!("******************     SECRET KEY        *******************\n");
    println!("{}\n", &wallet_info.secret_key().display_secret());
    println!("************************************************************\n");
    println!("************************************************************\n");
    println!("******************     PUBLIC KEY       ********************\n");
    println!("{}\n", &wallet_info.public_key().to_string());
    println!("************************************************************");
    println!("******************       ADDRESS         *******************");
    println!("*******  0x{}  *******", &wallet_info.address().encode_hex::<String>());
    println!("************************************************************\n");
}
