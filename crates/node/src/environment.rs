use lazy_static::lazy_static;
pub struct Environment {
    pub secret_key: String,
    pub blocks_processed_path: String,
    pub batch_interval: String,
    pub eo_contract_address: String,
    pub eth_rpc_url: String,
    pub eigenda_server_address: Option<String>,
    pub compute_rpc_url: String,
    pub storage_rpc_url: String,
    pub port: String,
    pub vipfs_address: Option<String>,
}

fn get_var_or_err(var: &str, err_buf: &mut String) -> String {
    match std::env::var(var) {
        Ok(val) => val,
        Err(_) => {
            err_buf.push_str(var);
            err_buf.push_str(", ");
            String::new()
        }
    }
}

impl Environment {
    pub fn new() -> Self {
        let mut err_buf = String::new();
        let secret_key = get_var_or_err("SECRET_KEY", &mut err_buf);
        let blocks_processed_path = get_var_or_err("BLOCKS_PROCESSED_PATH", &mut err_buf);
        let batch_interval = get_var_or_err("BATCH_INTERVAL", &mut err_buf);
        let eo_contract_address = get_var_or_err("EO_CONTRACT_ADDRESS", &mut err_buf);
        let eth_rpc_url = get_var_or_err("ETH_RPC_URL", &mut err_buf);
        let eigenda_server_address = std::env::var("EIGENDA_SERVER_ADDRESS").ok();

        let compute_rpc_url = get_var_or_err("COMPUTE_RPC_URL", &mut err_buf);
        let storage_rpc_url = get_var_or_err("STORAGE_RPC_URL", &mut err_buf);
        let port = std::env::var("PORT").unwrap_or_else(|_| "9292".to_string());
        let vipfs_address = std::env::var("VIPFS_ADDRESS").ok();

        if !err_buf.is_empty() {
            panic!("Environment variables missing: {}", err_buf);
        }
        Self {
            secret_key,
            blocks_processed_path,
            batch_interval,
            eo_contract_address,
            eth_rpc_url,
            eigenda_server_address,
            compute_rpc_url,
            storage_rpc_url,
            port,
            vipfs_address,
        }
    }
}

lazy_static! {
    pub static ref ENVIRONMENT: Environment = Environment::new();
}
