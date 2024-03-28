#![allow(unused)]
use async_recursion::async_recursion;
use bip39::{Language, Mnemonic};
use clap::ValueHint;
use clap::{
    command,
    error::{ContextKind, ContextValue, ErrorKind},
    value_parser, Arg, ArgAction, ArgGroup, ArgMatches, Command, Parser, Subcommand, ValueEnum,
};
use ethereum_types::Address as EthereumAddress;
use hex::ToHex;
use jsonrpsee::{
    core::client::ClientT,
    http_client::{HttpClient, HttpClientBuilder},
};
use lasr_compute::{
    LasrContentType, LasrObject, LasrObjectBuilder, LasrObjectCid, LasrObjectPayloadBuilder,
    LasrPackage, LasrPackageBuilder, LasrPackagePayloadBuilder, LasrPackageType, SignableObject,
};
use lasr_rpc::LasrRpcClient;
use lasr_types::{
    Account, AccountType, Address, BurnInstruction, CreateInstruction, HexOr20Bytes, HexOr32Bytes,
    Instruction, Namespace, Outputs, PayloadBuilder, Transaction, TransferInstruction,
    UpdateInstruction, U256,
};
use lasr_wallet::{Wallet, WalletBuilder, WalletInfo};
use secp256k1::PublicKey;
use secp256k1::{rand::rngs::OsRng, Keypair, Secp256k1, SecretKey};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::net::{AddrParseError, SocketAddr};
use std::path::PathBuf;
use std::{path::Path, str::FromStr};
use walkdir::WalkDir;
use web3_pkg::web3_store::Web3Store;

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

#[derive(Clone, Debug, ValueEnum)]
enum PackageType {
    Runtime,
    Program,
    Content,
}

#[derive(Clone, Debug)]
struct U256Wrapper(pub U256);

impl FromStr for U256Wrapper {
    type Err = uint::FromDecStrErr;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(U256Wrapper(ethereum_types::U256::from_dec_str(s)?.into()))
    }
}

impl std::ops::Mul<&Unit> for U256 {
    type Output = U256;
    fn mul(self, rhs: &Unit) -> Self::Output {
        match rhs {
            Unit::Echo => (self * U256::from(1_u128)),
            Unit::Beat => (self * U256::from(1_000_u128)),
            Unit::Note => (self * U256::from(1_000_000_u128)),
            Unit::Chord => (self * U256::from(1_000_000_000_u128)),
            Unit::Harmony => (self * U256::from(1_000_000_000_000_u128)),
            Unit::Melody => (self * U256::from(1_000_000_000_000_000_u128)),
            Unit::Verse => (self * U256::from(1_000_000_000_000_000_000_u128)),
        }
    }
}

fn parse_u256(s: &str) -> Result<U256, String> {
    Ok(ethereum_types::U256::from_dec_str(s)
        .map_err(|e| format!("Failed to parse U256: {}", e))?
        .into())
}

#[derive(Debug, Subcommand)]
enum Commands {
    BridgeOut,
    GetBalance {
        address: String,
        program_address: String,
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
        .arg(
            keypair_json_arg()
                .required(false)
        )
}

fn handle_new_wallet_command(children: &ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let values = children.ids();
    let seed = children.get_one::<u128>("seed");
    let passphrase = children.get_one::<String>("passphrase");
    let size = children.get_one::<usize>("mnemonic-size");
    let wallet_info =
        Wallet::<HttpClient>::new(seed, passphrase, size).expect("Unable to acquire WalletInfo");

    if let Some(flag) = children.get_one::<bool>("keypair-json") {
        pretty_print_keypair_info(&wallet_info);
    } else {
        pretty_print_wallet_info(&wallet_info);
    }

    let save = children.get_one::<bool>("save");
    if let Some(true) = save {
        let path = std::path::Path::new(
            children
                .get_one::<String>("path")
                .expect("required or default"),
        );
        let dir_path = path.parent().expect("dir has no parent");
        if !dir_path.exists() {
            std::fs::create_dir_all(dir_path).expect("unable to create directory for keyfile");
        }
        let overwrite = children
            .get_one::<bool>("overwrite")
            .expect("required or default");
        let mut file_buffer = Vec::new();
        let mut existing = if path.exists() {
            std::fs::File::open(path)
                .expect("file cant be opened")
                .read_to_end(&mut file_buffer)
                .expect("file should exist");
            if !file_buffer.is_empty() {
                serde_json::from_slice::<Vec<WalletInfo>>(&file_buffer)
                    .expect("keypair file has been corrupted")
            } else {
                Vec::new()
            }
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
                .as_bytes(),
        )
        .expect("unable to write wallet info to file");
    }

    Ok(())
}

fn from_mnemonic_command() -> Command {
    Command::new("mnemonic")
        .about("creates/loads a wallet keypair from a mnemonic")
        .arg_required_else_help(true)
        .arg(mnemonic_arg().required(true))
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

fn keypair_json_arg() -> Arg {
    Arg::new("keypair-json")
        .aliases(["print-keypair", "ppk", "json-keypair", "kp-json"])
        .long("keypair-json")
        .short('k')
        .action(ArgAction::SetTrue)
        .required(false)
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
        .aliases([
            "fsk",
            "fromsecretkey",
            "from-sk",
            "fromsk",
            "frompk",
            "fpk",
            "from-private-key",
            "fromprivatekey",
        ])
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
        .aliases([
            "sk",
            "pk",
            "secretkey",
            "secret_key",
            "private-key",
            "privatekey",
            "private_key",
        ])
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

fn cid_arg() -> Arg {
    Arg::new("content-id")
        .short('c')
        .long("cid")
        .aliases([
            "content-id",
            "contentid",
            "content_id",
            "contentId",
            "ipfs-id",
        ])
        .required(true)
        .help("a Base58btc encoded string representing the hash of a web3 package payload")
}

fn payload_arg() -> Arg {
    Arg::new("payload")
        .short('p')
        .long("payload")
        .aliases([
            "sign-me",
            "john-hancock",
            "siggy",
            "curvy",
            "gettin-siggy-wit-it",
        ])
        .required(true)
        .help("a payload to hash, turn into a signable message, and sign")
}

fn recursive_packer_arg() -> Arg {
    Arg::new("recursive")
        .short('r')
        .long("recursive")
        .help("whether or not you want to publish an entire directory")
        .action(ArgAction::SetTrue)
        .default_value("false")
        .value_parser(value_parser!(bool))
}

fn file_count_arg() -> Arg {
    Arg::new("file-count")
        .long("file-count")
        .aliases(["nf", "numfiles", "nfiles", "n-files", "number-of-files"])
        .help(
            "the number of files expected, not required can be inferred, but helps for validation",
        )
        .required(false)
        .default_value("0")
        .value_parser(value_parser!(u32))
}

fn package_path_arg() -> Arg {
    Arg::new("package-path")
        .long("package-path")
        .aliases([
            "pp",
            "ppath",
            "pubpath",
            "p-path",
            "pub-path",
            "pack",
            "pack-path",
        ])
        .help("the path to the package/object or directory to recursively get files from")
        .required(true)
}

fn ignore_file_arg() -> Arg {
    Arg::new("ignore-file")
        .long("ignore-file")
        .short('g')
        .aliases(["ignore", "ig"])
        .help("path to ignore file, i.e. the file that tells the app which components of a package/object/files in the directory to ignore")
        .required(false)
}

fn wizard_arg() -> Arg {
    Arg::new("wizard")
        .long("wizard")
        .aliases(["wiz", "guided", "pick"])
        .required(false)
        .action(ArgAction::SetTrue)
        .default_value("false")
        .value_parser(value_parser!(bool))
}

fn cids_arg() -> Arg {
    Arg::new("cids")
        .long("cids")
        .short('c')
        .help("the content ID of an object in web3 store to include, will pull from web3 store if included")
        .required(false)
}

fn content_type_arg() -> Arg {
    Arg::new("content-type")
        .long("content-type")
        .short('t')
        .help("The content type of package")
        .default_value("program")
        .required(true)
}

fn verbose_arg() -> Arg {
    Arg::new("verbose")
        .short('v')
        .long("verbose")
        .aliases(["vbs", "verb", "verbosity", "print"])
        .value_parser(value_parser!(bool))
        .action(ArgAction::SetTrue)
        .default_value("false")
}

fn api_version_arg() -> Arg {
    Arg::new("api-version")
        .long("api-version")
        .aliases(["av", "apiv", "vapi"])
        .required(false)
        .default_value("1")
        .value_parser(value_parser!(u32))
        .help("The api version of the package")
}

fn annotations_file_arg() -> Arg {
    Arg::new("annotations-file")
        .long("annotations-file")
        .aliases(["af", "af-path", "ann"])
        .help("path to the annotations file, should be JSON, YAML or TOML")
        .required(false)
}

fn package_name_arg() -> Arg {
    Arg::new("name").long("name").required(true)
}

fn author_arg() -> Arg {
    Arg::new("author")
        .short('a')
        .long("author")
        .aliases(["owner", "writer", "publisher", "dev"])
        .required(false)
}

fn package_version_arg() -> Arg {
    Arg::new("package-version")
        .long("package-version")
        .help("the version number of the package")
        .default_value("1")
        .value_parser(value_parser!(u32))
        .required(false)
}

fn runtime_arg() -> Arg {
    Arg::new("runtime")
        .long("runtime")
        .default_value("none")
        .required(false)
        .help("the runtime for a program or runtime package")
}

fn replaces_arg() -> Arg {
    Arg::new("replaces")
        .long("replaces")
        .required(false)
        .help("the cid of a package that this new package is intended to replace")
}

fn local_node_arg() -> Arg {
    Arg::new("local")
        .long("local")
        .help("is the web3 store a local node or remote node")
        .required(false)
        .default_value("false")
        .value_parser(value_parser!(bool))
}

fn entrypoint_arg() -> Arg {
    Arg::new("entrypoint")
        .short('e')
        .long("entrypoint")
        .aliases(["ep", "entrypoint", "entry-point", "exectuable"])
        .required(false)
        .default_value("")
}

fn program_args() -> Arg {
    Arg::new("program-args")
        .long("program-args")
        .aliases(["args", "commands", "at-start"])
        .required(false)
        .value_delimiter(',')
        .action(ArgAction::Append)
        .num_args(0..)
        .default_value("")
}

fn remote_node_arg() -> Arg {
    Arg::new("remote")
        .long("remote")
        .help("the domain or address of the remote web3 store node")
        .required_unless_present("local")
}

fn sign_command() -> Command {
    Command::new("sign")
        .aliases(["sign-transaction", "stx"])
        .arg(from_file_arg())
        .arg(keyfile_path_arg())
        .arg(wallet_index_arg())
        .arg(from_mnemonic_arg())
        .arg(mnemonic_arg())
        .arg(from_secret_key_arg())
        .arg(secret_key_arg())
        .arg(payload_arg())
        .arg(verbose_arg())
}

async fn handle_sign_command(children: &ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let values = children.ids();
    let mut wallet = get_wallet(children).await?;
    let payload = children.get_one::<String>("payload").expect("required");
    let sig = wallet
        .sign_payload(payload)
        .await
        .expect("unable to sign payload");
    println!("{}", serde_json::to_string_pretty(&sig).unwrap());
    Ok(())
}

fn publish_command() -> Command {
    Command::new("publish")
        .about("packages, signs, and publishes a package to web3 store.")
        .aliases([
            "pub",
            "push",
            "deploy",
            "ship",
            "release",
            "dist",
            "share",
            "post",
            "upload",
            "distribute",
        ])
        .arg(from_file_arg())
        .arg(keyfile_path_arg())
        .arg(wallet_index_arg())
        .arg(from_secret_key_arg())
        .arg(secret_key_arg())
        .arg(from_mnemonic_arg())
        .arg(mnemonic_arg())
        .arg(recursive_packer_arg())
        .arg(file_count_arg())
        .arg(package_path_arg())
        .arg(ignore_file_arg())
        .arg(wizard_arg())
        .arg(cids_arg())
        .arg(content_type_arg())
        .arg(api_version_arg())
        .arg(annotations_file_arg())
        .arg(author_arg())
        .arg(package_name_arg())
        .arg(package_version_arg())
        .arg(runtime_arg())
        .arg(local_node_arg())
        .arg(remote_node_arg())
        .arg(replaces_arg())
        .arg(entrypoint_arg())
        .arg(program_args())
        .arg(verbose_arg())
}

async fn handle_publish_command(children: &ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let values = children.ids();
    let wizard = children
        .get_one::<bool>("wizard")
        .expect("required or default");
    if *wizard {
        // build wizard struct and run it
        println!("The Wizard has not yet discovered his powers... Give him time to mature");
    } else {
        let (sk, _) = get_keypair(children)?;
        let wallet = get_wallet(children).await?;
        let package_path = children
            .get_one::<String>("package-path")
            .expect("package path is required");
        let ignore_file = children.get_one::<String>("ignore-file");
        let recursive = children
            .get_one::<bool>("recursive")
            .expect("required or default");
        let file_count = children
            .get_one::<u32>("file-count")
            .expect("required or default");
        let cids = children.get_one::<Vec<String>>("cids");
        let content_type = children
            .get_one::<String>("content-type")
            .expect("content type is required");
        let api_version = children
            .get_one::<u32>("api-version")
            .expect("required or default");
        let annotations_file = children.get_one::<String>("annotations-file");
        let author = children
            .get_one::<String>("author")
            .expect("required or default");
        let name = children.get_one::<String>("name").expect("required");
        let package_version = children
            .get_one::<u32>("package-version")
            .expect("required or default");
        let is_local = children
            .get_one::<bool>("local")
            .expect("required or default");
        let remote = children.get_one::<String>("remote");
        let runtime = children
            .get_one::<String>("runtime")
            .expect("required or default");
        let replaces = children.get_one::<String>("replaces");
        let entrypoint = children.get_one::<String>("entrypoint");
        let program_args = children.get_many::<String>("program-args");
        let verbose = children
            .get_one::<bool>("verbose")
            .expect("required or default");
        if *verbose {
            println!("gathered all items")
        };
        let store = if *is_local {
            Web3Store::local()?
        } else {
            let addr = remote
                .expect("remote node is required if not using --local flag")
                .to_string();
            let socket_addr: Result<SocketAddr, AddrParseError> = addr.parse();
            if let Ok(qualified_addr) = socket_addr {
                if *verbose {
                    println!("parsed ip address into SocketAddr")
                };
                let (ip_protocol, ip) = match qualified_addr.ip() {
                    std::net::IpAddr::V4(ip) => ("ip4".to_string(), ip.to_string()),
                    std::net::IpAddr::V6(ip) => ("ip6".to_string(), ip.to_string()),
                };
                let port = qualified_addr.port().to_string();

                let multiaddr_string = format!("/{ip_protocol}/{ip}/tcp/{port}");

                if *verbose {
                    println!("built multiaddr: {} out of SocketAddr", &multiaddr_string)
                };
                Web3Store::from_multiaddr(&multiaddr_string)?
            } else {
                Web3Store::from_hostname(&addr, true)?
            }
        };

        if *verbose {
            println!("built Web3Store")
        };

        let annots: BTreeMap<String, String> = if let Some(annotations_file) = annotations_file {
            let mut map_string = String::new();
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(false)
                .truncate(false)
                .append(false)
                .create(false)
                .open(annotations_file)?
                .read_to_string(&mut map_string);
            if let Ok(map) = serde_json::from_str(&map_string) {
                if *verbose {
                    println!("acquired annotations {:?}", &map)
                };
                map
            } else {
                BTreeMap::new()
            }
        } else {
            BTreeMap::new()
        };

        let mut package_builder = LasrPackagePayloadBuilder::default();
        let (cids, lasr_objects) = if cids.is_none() {
            let mut cids = Vec::new();
            let mut lasr_objects = Vec::new();
            recursively_publish_objects(
                package_path,
                &mut cids,
                &mut lasr_objects,
                &store,
                annots.clone(),
                sk,
                verbose,
            )
            .await?;

            (cids, lasr_objects)
        } else {
            let cids = cids
                .expect("cids should not be None at this point")
                .to_vec();
            (cids.clone(), get_lasr_objects(cids.clone()).await)
        };

        package_builder
            .api_version(*api_version)
            .package_name(name.to_string())
            .package_type(LasrPackageType::from((
                content_type.as_str(),
                runtime.as_str(),
            )))
            .package_author(author.to_string())
            .package_version(*package_version)
            .package_objects(lasr_objects);

        if let Some(replaces) = replaces {
            if let Ok(r) = serde_json::from_str(replaces) {
                package_builder.package_replaces(r);
            } else {
                package_builder.package_replaces(vec![]);
            }
        } else {
            package_builder.package_replaces(vec![]);
        }

        if let Some(entrypoint) = entrypoint {
            package_builder.package_entrypoint(entrypoint.clone().to_string());
        } else {
            package_builder.package_entrypoint("/".to_string());
        }

        if let Some(program_args) = program_args {
            package_builder.package_program_args(program_args.into_iter().cloned().collect());
        } else {
            package_builder.package_program_args(vec![]);
        }

        package_builder.package_annotations(annots);
        let package_payload = package_builder
            .build()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        let package_sig = package_payload.sign(&sk)?;
        let package = LasrPackage {
            package_payload,
            package_sig,
        };

        let package_json = serde_json::to_string(&package).unwrap();
        let package_cid = store.write_dag(package_json.into()).await?;

        println!("{}", package_cid);
    }

    Ok(())
}

async fn get_lasr_objects(cids: Vec<String>) -> Vec<LasrObject> {
    vec![]
}

#[async_recursion]
async fn recursively_publish_objects(
    package_path: &String,
    cid_buffer: &mut Vec<String>,
    objects: &mut Vec<LasrObject>,
    store: &Web3Store,
    annotations: BTreeMap<String, String>,
    sk: SecretKey,
    verbose: &bool,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in WalkDir::new(package_path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() {
            if *verbose {
                println!("writing {} to Web3Store", path.to_string_lossy());
            }
            let cid = store.write_object(std::fs::read(path)?).await?;
            cid_buffer.push(cid.clone());

            if *verbose {
                println!(
                    "published {} to Web3Store, CID: {}",
                    path.to_string_lossy(),
                    &cid
                );
            }
            let mut payload_builder = LasrObjectPayloadBuilder::default()
                .object_cid(LasrObjectCid::from(cid.clone()))
                .object_path(path.to_string_lossy().to_string())
                .object_content_type(LasrContentType::from(PathBuf::from(path)))
                .clone();

            if let Some(inner_annots) = annotations.get(&path.to_string_lossy().to_string()) {
                if let Ok(annots) = serde_json::from_str(inner_annots) {
                    payload_builder.object_annotations(annots);
                }
            }

            let object_payload = payload_builder.build()?;
            let object_sig = object_payload.sign(&sk)?;
            let object = LasrObject {
                object_payload,
                object_sig,
            };
            if *verbose {
                println!(
                    "Successfully published: {}, CID: {}",
                    path.to_string_lossy(),
                    &cid
                )
            };
            objects.push(object);
        }
    }
    Ok(())
}

fn register_program_command() -> Command {
    Command::new("register-program")
        .aliases([
            "deploy",
            "rp",
            "register",
            "reg-prog",
            "prog",
            "deploy-prog",
            "register-contract",
            "deploy-contract",
            "rc",
            "dc",
        ])
        .arg(from_file_arg())
        .arg(keyfile_path_arg())
        .arg(wallet_index_arg())
        .arg(from_mnemonic_arg())
        .arg(mnemonic_arg())
        .arg(from_secret_key_arg())
        .arg(secret_key_arg())
        .arg(cid_arg())
        .arg(verbose_arg())
}

async fn handle_register_program(children: &ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let verbose = *children
        .get_one::<bool>("verbose")
        .expect("required or default");
    if verbose {
        dbg!("Getting wallet");
    }
    let mut wallet = get_wallet(children).await?;
    let address = wallet.address();
    if verbose {
        println!("Wallet address: {:?}", address);
    }
    if verbose {
        dbg!("Getting account");
    }
    let cid = children.get_one::<String>("content-id").expect("required");
    let inputs = json!({"contentId": cid});
    if verbose {
        dbg!("Inputs obtained: {:?}", inputs.to_string());
    }
    if verbose {
        dbg!("Registering program");
    }
    let program_address = wallet
        .register_program(&inputs.to_string())
        .await
        .map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )) as Box<dyn std::error::Error>
        })?;
    if verbose {
        dbg!("Program registered");
    }
    if verbose {
        dbg!("Getting account again");
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({"program_address": program_address}))
            .unwrap()
    );

    wallet.get_account(&address).await;
    if verbose {
        dbg!("Account obtained again");
    }
    Ok(())
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
        .arg(verbose_arg())
}

async fn handle_call_command(children: &ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let values = children.ids();
    let mut wallet = get_wallet(children).await?;
    let address = wallet.address();
    let verbose = children
        .get_one::<bool>("verbose")
        .expect("required or default");
    if *verbose {
        dbg!("attempting to get account");
    }
    wallet.get_account(&address).await;

    if *verbose {
        dbg!("parsing cli args");
    }
    let to = children.get_one::<Address>("to").expect("required");
    let pid = children
        .get_one::<Address>("content-namespace")
        .expect("required");
    let value = children.get_one::<U256Wrapper>("value").expect("required");
    let op = children.get_one::<String>("op").expect("required");
    let inputs = children.get_one::<String>("inputs").expect("required");
    let unit = children.get_one::<Unit>("unit").expect("required");

    let amount = value.0 * unit;

    if *verbose {
        dbg!("submitting transaction");
    }
    let tx_hash_string = wallet
        .call(pid, to, amount, op, inputs)
        .await
        .map_err(|e| e as Box<dyn std::error::Error>)?;
    println!("{}", tx_hash_string);
    Ok(())
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
        .arg(verbose_arg())
}

async fn handle_send_command(children: &ArgMatches) -> Result<(), Box<dyn ::std::error::Error>> {
    println!("received `wallet send` command");
    let values = children.ids();
    let mut wallet = get_wallet(children).await?;

    let address = wallet.address();

    wallet.get_account(&address).await;

    let to = children.get_one::<Address>("to").expect("required");
    let pid = children
        .get_one::<Address>("content-namespace")
        .expect("required");
    let value = children.get_one::<U256Wrapper>("value").expect("required");
    let unit = children.get_one::<Unit>("unit").expect("required");

    let amount = value.0 * unit;

    let token = wallet.send(to, pid, amount).await.map_err(|e| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        )) as Box<dyn std::error::Error>
    })?;

    println!("{}", serde_json::to_string_pretty(&token)?);

    Ok(())
}

fn address_arg() -> Arg {
    Arg::new("address")
        .long("address")
        .short('a')
        .aliases(["pubkey", "addy", "addr"])
}

fn get_account_command() -> Command {
    Command::new("get-account")
        .about("gets the account given a secret key or mnemonic")
        .arg(address_arg())
        .arg(verbose_arg())
}

async fn handle_get_account_command(
    children: &ArgMatches,
) -> Result<(), Box<dyn std::error::Error>> {
    use lasr_rpc::LasrRpcClient;
    let address = children
        .get_one::<String>("address")
        .expect("address is required");
    let client: HttpClient = get_client().await?;
    let account = client.get_account(address.clone()).await?;
    println!("{}", account);
    Ok(())
}

fn wallet_command() -> Command {
    Command::new("wallet")
        .about("a wallet cli for interacting with the LASR network as a user")
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(new_wallet_command())
        .subcommand(from_mnemonic_command())
        .subcommand(from_secret_key_command())
        .subcommand(send_command())
        .subcommand(call_command())
        .subcommand(register_program_command())
        .subcommand(get_account_command())
        .subcommand(sign_command())
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
    Arg::new("json").short('j').long("json")
}

fn parse_outputs() -> Command {
    Command::new("parse-outputs")
        .arg(from_json_file_arg())
        .arg(json_arg())
}

fn handle_parse_outputs_command(children: &ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let values = children.ids();
    let from_file = children.get_one::<bool>("from-file");
    if let Some(true) = from_file {
        let json = children
            .get_one::<String>("json")
            .expect("unable to acquire json filepath");
        let mut file = OpenOptions::new()
            .read(true)
            .write(false)
            .append(false)
            .truncate(false)
            .create_new(false)
            .open(json)
            .expect("unable to open json file");

        let mut json_str = String::new();
        file.read_to_string(&mut json_str)
            .expect("unable to read json file contents to string");
        let outputs: Outputs = serde_json::from_str(&json_str)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        println!("{:#?}", outputs);
    } else {
        let json_str = children
            .get_one::<String>("json")
            .expect("unable to acquire json from command line");
        let outputs: Outputs = serde_json::from_str(json_str)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        println!("{:#?}", outputs);
    }

    Ok(())
}

fn parse_transaction() -> Command {
    Command::new("parse-transaction")
        .aliases(["parse-tx", "parse-tx-json"])
        .arg(json_arg())
}

fn handle_parse_transaction_command(
    children: &ArgMatches,
) -> Result<(), Box<dyn std::error::Error>> {
    let value = children.ids();
    let json_str = children
        .get_one::<String>("json")
        .expect("unable to acquire json from command line");
    let transaction: Transaction =
        serde_json::from_str(json_str).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    println!("{:#?}", transaction);
    Ok(())
}

fn get_default_transaction() -> Command {
    Command::new("default-transaction").aliases(["default-tx", "tx"])
}

fn hex_or_bytes() -> Command {
    Command::new("hex-or-bytes")
}

fn handle_hex_or_bytes_command(children: &ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let hex = HexOr20Bytes::Hex(hex::encode([2; 20]));
    let bytes = HexOr20Bytes::Bytes([2; 20]);
    println!(
        "{:?}",
        serde_json::to_string(&hex).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
    );
    println!(
        "{:?}",
        serde_json::to_string(&bytes).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
    );
    Ok(())
}

fn instruction() -> Command {
    Command::new("instruction")
}

fn handle_instruction_command(children: &ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let create = Instruction::Create(CreateInstruction::default());
    let update = Instruction::Update(UpdateInstruction::default());
    let transfer = Instruction::Transfer(TransferInstruction::default());
    let burn = Instruction::Burn(BurnInstruction::default());

    let create_json =
        serde_json::to_string(&create).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    let update_json =
        serde_json::to_string(&update).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    let transfer_json =
        serde_json::to_string(&transfer).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    let burn_json =
        serde_json::to_string(&burn).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    println!("{:?}", create_json);
    println!("{:?}", update_json);
    println!("{:?}", transfer_json);
    println!("{:?}", burn_json);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = command!()
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .about("A cli interacting with the LASR network")
        .subcommand(wallet_command())
        .subcommand(parse_outputs())
        .subcommand(parse_transaction())
        .subcommand(get_default_transaction())
        .subcommand(hex_or_bytes())
        .subcommand(instruction())
        .subcommand(publish_command())
        .get_matches();

    match matches.subcommand() {
        Some(("wallet", sub)) => match sub.subcommand() {
            Some(("new", children)) => {
                handle_new_wallet_command(children)?;
            }
            Some(("mnemonic", children)) => {
                println!("received `wallet mnemonic` command");
            }
            Some(("secret-key", children)) => {
                println!("received `wallet secret-key` command");
            }
            Some(("sign", children)) => {
                handle_sign_command(children).await?;
            }
            Some(("send", children)) => {
                handle_send_command(children).await?;
            }
            Some(("call", children)) => {
                handle_call_command(children).await?;
            }
            Some(("register-program", children)) => {
                handle_register_program(children).await?;
            }
            Some(("get-account", children)) => {
                handle_get_account_command(children).await?;
            }
            _ => {}
        },
        Some(("parse-outputs", children)) => {
            handle_parse_outputs_command(children)?;
        }
        Some(("parse-transaction", children)) => {
            handle_parse_transaction_command(children)?;
        }
        Some(("default-transaction", children)) => {
            println!(
                "{:?}",
                serde_json::to_string(&Transaction::default())
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
            );
        }
        Some(("hex-or-bytes", children)) => {
            handle_hex_or_bytes_command(children)?;
        }
        Some(("instruction", children)) => {
            handle_instruction_command(children)?;
        }
        Some(("publish", children)) => {
            handle_publish_command(children).await?;
        }
        _ => {}
    }

    Ok(())
}

fn get_keypair(
    children: &ArgMatches,
) -> Result<(SecretKey, PublicKey), Box<dyn std::error::Error>> {
    let values = children.ids();

    let from_file = children.get_flag("from-file");
    let from_mnemonic = children.get_flag("from-mnemonic");
    let from_secret_key = children.get_flag("from-secret-key");

    if from_file {
        let keypair_file = children
            .get_one::<String>("path")
            .expect("required if from-file flag");
        let wallet_index = children
            .get_one::<usize>("wallet-index")
            .expect("required or default");
        let mut file = std::fs::File::open(keypair_file)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents);
        let keypair_file: Vec<WalletInfo> = serde_json::from_str(&contents)?;
        let wallet_info = &keypair_file[*wallet_index];
        let master = wallet_info.secret_key();
        let pubkey = wallet_info.public_key();
        return Ok((master, pubkey));
    } else if from_mnemonic {
        let phrase = children
            .get_one::<String>("mnemonic")
            .expect("required if from-mnemonic flag");
        let mnemonic = Mnemonic::parse_in_normalized(Language::English, phrase)?;
        let seed = mnemonic.to_seed("");
        let secp = Secp256k1::new();
        let master = SecretKey::from_slice(&seed[0..32])?;
        let pubkey = master.public_key(&secp);
        return Ok((master, pubkey));
    } else {
        let sk = children
            .get_one::<String>("secret-key")
            .expect("required if from-secret-key flag");
        let secp = Secp256k1::new();
        let master = SecretKey::from_str(sk)?;
        let pubkey = master.public_key(&secp);
        let address: Address = pubkey.into();
        let eaddr: EthereumAddress = address.into();
        let verbose = *children
            .get_one::<bool>("verbose")
            .expect("required or default");
        if verbose {
            println!("************************************************************");
            println!("************************************************************\n");
            println!("************************************************************");
            println!("******************     SECRET KEY        *******************\n");
            println!("{}\n", &sk);
            println!("************************************************************\n");
            println!("************************************************************");
            println!("******************       ADDRESS         *******************");
            println!("*******  0x{}  *******", &address.encode_hex::<String>());
            println!("************************************************************\n");
        }
        return Ok((master, pubkey));
    }

    Err(Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        "unable to derive secret and public key",
    )))
}

async fn get_wallet(
    children: &ArgMatches,
) -> Result<Wallet<HttpClient>, Box<dyn std::error::Error>> {
    let (secret_key, public_key) = get_keypair(children)?;
    let address: Address = public_key.into();
    let lasr_rpc_url =
        std::env::var("LASR_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:9292".to_string());
    let client = HttpClientBuilder::default().build(lasr_rpc_url)?;
    let res = &client.get_account(format!("{:x}", address)).await;
    let account = if let Ok(account_str) = res {
        println!("{}", account_str);
        let account: Account = serde_json::from_str(account_str)?;
        account
    } else {
        Account::new(AccountType::User, None, address, None)
    };

    WalletBuilder::default()
        .sk(secret_key)
        .client(client.clone())
        .address(address)
        .builder(PayloadBuilder::default())
        .account(account)
        .build()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

async fn get_client() -> Result<HttpClient, Box<dyn std::error::Error>> {
    let lasr_rpc_url =
        std::env::var("LASR_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:9292".to_string());
    let client = HttpClientBuilder::default().build(lasr_rpc_url)?;
    Ok(client)
}

fn pretty_print_keypair_info(wallet_info: &WalletInfo) {
    println!("{}", serde_json::to_string_pretty(wallet_info).unwrap())
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
    println!(
        "*******  0x{}  *******",
        &wallet_info.address().encode_hex::<String>()
    );
    println!("************************************************************\n");
}
