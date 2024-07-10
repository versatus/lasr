use crate::{LasrContentType, LasrObjectRuntime, LasrPackage, LasrPackageType, ProgramFormat};
use derive_builder::Builder;
use lasr_messages::{ActorType, ExecutorMessage};
use lasr_types::{Inputs, ProgramSchema, Transaction};
use oci_spec::runtime::{ProcessBuilder, RootBuilder, Spec};
use ractor::ActorRef;
use std::io::Read;
use std::io::Write;
use std::os::unix::prelude::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::{ffi::OsStr, fmt::Display};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use web3_pkg::web3_store::Web3Store;

#[allow(unused)]
use ipfs_api::{IpfsApi, IpfsClient};

const IPFS_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub enum BaseImage {
    Wasm,
    Bin,
    Python,
    Node,
    Bun,
    Java,
}

impl From<LasrObjectRuntime> for BaseImage {
    fn from(value: LasrObjectRuntime) -> Self {
        match value {
            LasrObjectRuntime::Bin => BaseImage::Bin,
            LasrObjectRuntime::Wasm => BaseImage::Wasm,
            LasrObjectRuntime::Node => BaseImage::Node,
            LasrObjectRuntime::Python => BaseImage::Python,
            LasrObjectRuntime::Bun => BaseImage::Bun,
            LasrObjectRuntime::Java => BaseImage::Java,
            LasrObjectRuntime::Other(_) => BaseImage::Bin,
            LasrObjectRuntime::None => BaseImage::Bin,
        }
    }
}

impl Display for BaseImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BaseImage::Bin => write!(f, "bin"),
            BaseImage::Wasm => write!(f, "wasm"),
            BaseImage::Python => write!(f, "python"),
            BaseImage::Node => write!(f, "node"),
            BaseImage::Java => write!(f, "java"),
            BaseImage::Bun => write!(f, "bun"),
        }
    }
}

impl BaseImage {
    pub fn path(&self) -> String {
        format!("./base_image/{}", self)
    }
}

#[derive(Debug)]
pub struct PackageContainerMetadata {
    base_image: BaseImage,
    cid: String,
    entrypoint: String,
    program_args: Vec<String>,
}

impl PackageContainerMetadata {
    pub fn new(
        base_image: BaseImage,
        cid: String,
        entrypoint: String,
        program_args: Vec<String>,
    ) -> Self {
        Self {
            base_image,
            cid,
            entrypoint,
            program_args,
        }
    }

    pub fn base_image(&self) -> &BaseImage {
        &self.base_image
    }

    pub fn cid(&self) -> &String {
        &self.cid
    }

    pub fn entrypoint(&self) -> &String {
        &self.entrypoint
    }

    pub fn program_args(&self) -> &Vec<String> {
        &self.program_args
    }
}

#[derive(Builder, Debug, Clone)]
pub struct OciManager {
    bundler: OciBundler<String, String>,
    store: Option<String>,
}

impl OciManager {
    pub fn new(bundler: OciBundler<String, String>, store: Option<String>) -> Self {
        Self { bundler, store }
    }

    pub fn try_get_store(&self) -> Result<Web3Store, std::io::Error> {
        let store = if let Some(addr) = &self.store {
            Web3Store::from_multiaddr(addr).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error aquiring Web3Store for {addr}: {e:?}"),
                )
            })?
        } else {
            Web3Store::local().map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error aquiring local Web3Store: {e:?}"),
                )
            })?
        };

        Ok(store)
    }

    pub async fn check_pinned_status(&self, content_id: &str) -> Result<(), std::io::Error> {
        tracing::info!(
            "calling self.store.is_pinned to check if {} is pinned",
            content_id
        );
        let store = self.try_get_store()?;
        match timeout(IPFS_TIMEOUT, store.is_pinned(content_id)).await {
            Ok(result) => result.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error checking pinned status for {content_id}: {e:?}"),
                )
            }),
            Err(_) => {
                tracing::error!("Timed out checking if {} is pinned", content_id);
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Timed out checking if content is pinned",
                ))
            }
        }
    }

    pub async fn pin_object(
        &self,
        content_id: &str,
        recursive: bool,
    ) -> Result<(), std::io::Error> {
        let store = self.try_get_store()?;
        let cids = match timeout(IPFS_TIMEOUT, store.pin_object(content_id, recursive)).await {
            Ok(result) => result.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error pinning {content_id}: {e:?}"),
                )
            })?,
            Err(_) => {
                tracing::error!("Timed out pinning object in IPFS");
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Timed out pinning object in IPFS",
                ));
            }
        };

        tracing::info!("Pinned object: {:?}", cids);
        Ok(())
    }

    pub async fn create_payload_package(
        &self,
        content_id: impl AsRef<Path>,
    ) -> Result<Option<PackageContainerMetadata>, std::io::Error> {
        let cid = content_id.as_ref().to_string_lossy().to_string();
        let payload_path_string = self
            .bundler
            .get_payload_path(content_id.as_ref())
            .as_ref()
            .to_string_lossy()
            .to_string();

        tracing::info!("Attempting to read DAG for {} from Web3Store...", &cid);
        let store = self.try_get_store().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to get Web3Store: {e:?}"),
            )
        })?;
        let package_data = match timeout(IPFS_TIMEOUT, store.read_dag(&cid)).await {
            Ok(Ok(data)) => data,
            Ok(Err(e)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failure to read DAG from Web3Store: {e:?}"),
                ))
            }
            Err(_) => {
                tracing::error!("Timed out reading from IPFS");
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Timed out reading from IPFS",
                ));
            }
        };

        let package_dir = format!("{}/{}", &payload_path_string, &cid);

        tracing::info!("creating all directories in path: {}", &package_dir);
        std::fs::create_dir_all(&package_dir).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Error creating payload directory for {cid} in path {package_dir}: {e:?}"),
            )
        })?;

        let package: LasrPackage = serde_json::from_slice(&package_data).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Error deserializing package from DAG {package_data:?}: {e:?}"),
            )
        })?;
        tracing::info!(
            "Package '{}' version {} from '{}' is type {:?}",
            &package.package_payload.package_name,
            &package.package_payload.package_version,
            &package.package_payload.package_author,
            &package.package_payload.package_type,
        );

        let container_metadata = match package.package_payload.package_type {
            LasrPackageType::Program(runtime) => Some(PackageContainerMetadata::new(
                BaseImage::from(runtime),
                cid.to_string(),
                package.package_payload.package_entrypoint,
                package.package_payload.package_program_args,
            )),
            _ => None,
        };

        let package_metadata_filepath = format!("{}/metadata.json", &package_dir);

        tracing::info!(
            "creating package metadata file: {}",
            &package_metadata_filepath
        );
        let mut f = std::fs::File::create(&package_metadata_filepath).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Error creating payload metadata directory in path {package_dir}: {e:?}"),
            )
        })?;

        f.write_all(&package_data).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Error writing to package metadata file {f:?}: {e:?}"),
            )
        })?;

        let package_object_iter = package.package_payload.package_objects.into_iter();

        //TODO(asmith) convert into a parallel iterator
        for obj in package_object_iter {
            tracing::info!("getting object: {} from Web3Store", &obj.object_cid());
            let object_data = store.read_object(obj.object_cid()).await.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to read for {obj:?} from Web3Store: {e:?}"),
                )
            })?;

            let mut object_path = obj
                .object_path()
                .strip_prefix("./payload/")
                .unwrap_or(obj.object_path());
            if object_path == obj.object_path() {
                object_path = obj
                    .object_path()
                    .strip_prefix("./")
                    .unwrap_or(obj.object_path());
            }

            let (object_filepath, exec) = match obj.object_content_type() {
                LasrContentType::Program(program_format) => match program_format {
                    ProgramFormat::Executable => {
                        (format!("{}/{}", &package_dir, object_path,), true)
                    }
                    ProgramFormat::Script(_) => {
                        (format!("{}/{}", &package_dir, object_path,), true)
                    }
                    ProgramFormat::Lib(_) => (format!("{}/{}", &package_dir, object_path), false),
                },
                LasrContentType::Document(_) => {
                    (format!("{}/{}", &package_dir, object_path), false)
                }
                LasrContentType::Image(_) => (format!("{}/{}", &package_dir, object_path), false),
                LasrContentType::Audio(_) => (format!("{}/{}", &package_dir, object_path), false),
                LasrContentType::Video(_) => (format!("{}/{}", &package_dir, object_path), false),
            };

            tracing::info!("creating missing directories in: {}", &object_filepath);
            let object_path = Path::new(&object_filepath);
            if let Some(parent) = object_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Error creating object filepath at {object_filepath} for {parent:?}: {e:?}"),
                    )
                })?;
            }
            tracing::info!("writing object to: {}", &object_filepath);

            let mut f = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .truncate(true)
                .append(false)
                .create(true)
                .open(&object_filepath)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Error creating object file in {object_filepath}: {e:?}"),
                    )
                })?;

            f.write_all(&object_data).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error writing to object file {f:?}: {e:?}"),
                )
            })?;

            if exec {
                let mut permissions = std::fs::metadata(object_path)
                    .map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to query files in {object_path:?}: {e:?}"),
                        )
                    })?
                    .permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(object_path, permissions).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Error setting permissions on files in {object_path:?}: {e:?}"),
                    )
                })?;
            }
        }

        Ok(container_metadata)
    }

    pub async fn bundle(&self, content_id: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let cid = content_id
            .as_ref()
            .to_string_lossy()
            .into_owned()
            .to_string();
        tracing::info!("attempting to create bundle for {}", cid);
        let container_metadata = self.create_payload_package(content_id).await.map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Error creating payload for {cid}: {e:?}"),
            )
        })?;
        if let Some(metadata) = container_metadata {
            tracing::info!("received container metadata: {:?}", &metadata);
            tracing::info!("building container bundle");
            self.bundler.bundle(&cid, &metadata).await.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error building container bundle for {cid}: {e:?}"),
                )
            })?;
            self.add_payload(&cid).await.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to add payload for {cid}: {e:?}"),
                )
            })?;
            self.base_spec(&cid).await?;
            let program_args = {
                if metadata.program_args().is_empty() {
                    None
                } else {
                    Some(metadata.program_args().clone())
                }
            };
            self.customize_spec(cid, &metadata, metadata.entrypoint(), program_args)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Error customizing container configuration: {e:?}"),
                    )
                })?;
            return Ok(());
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "BaseImage not found, unsuported runtime or content type",
        ))
    }

    pub async fn add_payload(&self, content_id: impl AsRef<Path>) -> Result<(), std::io::Error> {
        self.bundler.add_payload(content_id).await
    }

    pub async fn base_spec(&self, content_id: impl AsRef<Path>) -> Result<(), std::io::Error> {
        self.bundler.base_spec(content_id).await
    }

    pub fn customize_spec(
        &self,
        content_id: impl AsRef<Path>,
        container_metadata: &PackageContainerMetadata,
        entrypoint: &str,
        program_args: Option<Vec<String>>,
    ) -> Result<(), std::io::Error> {
        self.bundler
            .customize_spec(content_id, container_metadata, entrypoint, program_args)
    }

    pub fn get_program_schema(
        &self,
        content_id: impl AsRef<Path>,
    ) -> std::io::Result<ProgramSchema> {
        self.bundler.get_program_schema(content_id)
    }

    pub async fn run_container(
        &self,
        content_id: impl AsRef<Path> + Send + 'static,
        program_id: String,
        transaction: Option<Transaction>,
        inputs: Inputs,
        transaction_hash: Option<String>,
    ) -> Result<tokio::task::JoinHandle<Result<String, std::io::Error>>, std::io::Error> {
        let container_path = self
            .bundler
            .get_container_path(&content_id)
            .as_ref()
            .to_string_lossy()
            .into_owned();

        let container_id = content_id.as_ref().to_string_lossy().into_owned();

        let inner_inputs = inputs.clone();
        tracing::warn!(
            "Calling: runsc --rootless --network=none run -bundle {} {}",
            &container_path,
            &container_id
        );
        Ok(tokio::spawn(async move {
            // Create temp file for this container to output to
            let temp_file_path = std::env::temp_dir().join(format!("lasr/{}.out", container_id));

            // Create the temporary file for container output
            if let Some(parent) = temp_file_path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Error creating temp conatiner path {temp_file_path:?}: {e:?}"),
                    )
                })?;
            }

            // Check if the file exists
            if tokio::fs::metadata(&temp_file_path).await.is_ok() {
                // If it exists, make sure it is empty
                tokio::fs::write(&temp_file_path, b"").await.map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Error writing container metadata to {temp_file_path:?}: {e:?}"),
                    )
                })?;
            }

            // Create a standard file for stdout redirection
            let container_stdout_file = std::fs::File::create(&temp_file_path).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "Error creating temp container file in path {temp_file_path:?}:  {e:?}"
                    ),
                )
            })?;

            let mut child = Command::new("runsc")
                .arg("--rootless")
                .arg("--network=none")
                .arg("run")
                .arg("-bundle")
                .arg(&container_path)
                .arg(&container_id)
                .stdin(Stdio::piped())
                .stdout(Stdio::from(container_stdout_file))
                .spawn()
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Error executing runsc command for {container_id}: {e:?}"),
                    )
                })?;

            let mut stdin = child.stdin.take().ok_or({
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "unable to acquire child stdin, compute.rs: 120".to_string(),
                )
            })?;
            let stdio_inputs = serde_json::to_string(&inner_inputs.clone())?;
            tracing::info!("passing inputs to stdio: {:#?}", &stdio_inputs);
            let _ = tokio::task::spawn(async move {
                stdin
                    .write_all(stdio_inputs.clone().as_bytes())
                    .await
                    .map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to write stdio inputes to stdin: {e:?}"),
                        )
                    })?;

                drop(stdin);
                Ok::<_, std::io::Error>(())
            })
            .await
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Unable to join async handle: {e:?}"),
                )
            })?;
            let status = child.wait().await.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error retrieving child status: {e:?}"),
                )
            })?;
            if !status.success() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "runsc command failed",
                ));
            }

            let mut file = tokio::fs::File::open(&temp_file_path).await.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to open {temp_file_path:?}: {e:?}"),
                )
            })?;
            let mut outputs = String::new();
            file.read_to_string(&mut outputs).await.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to parse {file:?} for output results: {e:?}"),
                )
            })?;

            if outputs.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "temporary file is empty after container execution",
                ));
            }

            tracing::warn!("result from container: {container_id} = {:#?}", outputs);

            let actor: ActorRef<ExecutorMessage> =
                ractor::registry::where_is(ActorType::Executor.to_string())
                    .ok_or(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "unable to acquire Executor actor from inside container execution thread",
                    ))?
                    .into();

            tracing::warn!("results received, informing executor");
            let message = ExecutorMessage::Results {
                content_id: content_id.as_ref().to_string_lossy().into_owned(),
                program_id,
                transaction_hash,
                transaction,
            };

            actor.cast(message).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to cast message to executor: {e:?}"),
                )
            })?;

            tracing::warn!("casted message to inform executor");

            // Clean up the temporary file
            tokio::fs::remove_file(&temp_file_path).await.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error removing temp container file {temp_file_path:?}: {e:?}"),
                )
            })?;

            Ok::<_, std::io::Error>(outputs)
        }))
    }
}
#[derive(Builder, Clone, Debug)]
pub struct OciBundler<R: AsRef<OsStr>, P: AsRef<Path>> {
    containers: P,
    #[allow(unused)]
    base_images: P,
    runtime: R,
    payload_path: P,
}

impl<R: AsRef<OsStr>, P: AsRef<Path>> OciBundler<R, P> {
    pub const CONTAINER_ROOT: &'static str = "rootfs";
    pub const CONTAINER_BIN: &'static str = "bin";

    pub fn new(runtime: R, containers: P, base_images: P, payload_path: P) -> Self {
        Self {
            containers,
            base_images,
            runtime,
            payload_path,
        }
    }

    pub async fn bundle(
        &self,
        content_id: impl AsRef<Path> + std::fmt::Debug,
        container_metadata: &PackageContainerMetadata,
    ) -> Result<(), std::io::Error> {
        let base_path = self.get_base_path(container_metadata.base_image());
        let container_path = self.get_container_path(&content_id);
        if !container_path.as_ref().exists() {
            tracing::info!(
                "container path: {} doesn't exist, creating...",
                container_path.as_ref().to_string_lossy().to_string()
            );
            std::fs::create_dir_all(container_path.as_ref()).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error creating container path for {content_id:?}: {e:?}"),
                )
            })?;
        }
        let container_root_path = self.container_root_path(&container_path);
        if !container_root_path.as_ref().exists() {
            tracing::info!(
                "container root path: {} doesn't exist, creating...",
                container_root_path.as_ref().to_string_lossy().to_string()
            );
            link_dir(
                &base_path.as_ref().join(Self::CONTAINER_ROOT),
                &container_root_path.as_ref(),
            )
            .await
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error creating container root path for {content_id:?}: {e:?}"),
                )
            })?;
        }

        Ok(())
    }

    pub async fn add_payload(&self, content_id: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let container_path = self.get_container_path(&content_id);
        let container_root = self.container_root_path(&container_path);
        let payload_path = self.get_payload_path(&content_id);
        tracing::info!(
            "Attempting to copy {:?} to {:?}",
            &payload_path.as_ref().canonicalize(),
            &container_root.as_ref().canonicalize()
        );
        if let Err(e) = copy_dir(payload_path, container_root).await {
            tracing::error!("Error adding payload: {e}");
        };

        Ok(())
    }

    pub async fn base_spec(&self, content_id: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let container_path = self.get_container_path(&content_id);
        Command::new(&self.runtime)
            .arg("spec")
            .current_dir(container_path)
            .output()
            .await
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error establishing runtime for container: {e:?}"),
                )
            })?;

        Ok(())
    }

    pub fn customize_spec(
        &self,
        content_id: impl AsRef<Path>,
        container_metadata: &PackageContainerMetadata,
        entrypoint: &str,
        program_args: Option<Vec<String>>,
    ) -> Result<(), std::io::Error> {
        let container_path = self.get_container_path(&content_id);
        let config_path = container_path.as_ref().join("config.json");

        let mut spec: Spec = Spec::load(&config_path).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to load spec config from {config_path:?}: {e:?}"),
            )
        })?;

        let mut proc = if let Some(gen_proc) = spec.process() {
            gen_proc.to_owned()
        } else {
            ProcessBuilder::default().build().map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Error while building default Oci Process: {e:?}"),
                )
            })?
        };

        match container_metadata.base_image() {
            BaseImage::Node => {
                let mut args = Vec::new();
                args.push("node".to_string());
                args.push(format!(
                    "/{}/{}/{}",
                    content_id.as_ref().display(),
                    content_id.as_ref().display(),
                    entrypoint
                ));

                let guest_env = vec!["PATH=/usr/local/bin".to_string(), "TERM=xterm".to_string()];
                proc.set_env(Some(guest_env));
                proc.set_args(Some(args));
                spec.set_process(Some(proc));

                let mut rootfs = if let Some(genroot) = spec.root() {
                    genroot.to_owned()
                } else {
                    RootBuilder::default().build().map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Error while building default Oci Root: {e:?}"),
                        )
                    })?
                };

                rootfs.set_path(std::path::PathBuf::from(Self::CONTAINER_ROOT));
                rootfs.set_readonly(Some(false));
                spec.set_root(Some(rootfs));

                std::fs::write(
                    &config_path,
                    serde_json::to_string_pretty(&spec).map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to serialize Oci spec: {e}"),
                        )
                    })?,
                )
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to write Oci spec to {config_path:?}: {e}"),
                    )
                })?;
            }
            _ => {
                let mut args = vec![format!(
                    "/{}/{}/{}/",
                    content_id.as_ref().display(),
                    content_id.as_ref().display(),
                    entrypoint
                )];
                if let Some(pargs) = program_args {
                    args.extend(pargs);
                }

                let guest_env = vec!["PATH=/bin".to_string(), "TERM=xterm".to_string()];
                proc.set_env(Some(guest_env));
                proc.set_args(Some(args));
                spec.set_process(Some(proc));

                let mut rootfs = if let Some(genroot) = spec.root() {
                    genroot.to_owned()
                } else {
                    RootBuilder::default().build().map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Error while building default Oci Root: {e:?}"),
                        )
                    })?
                };

                rootfs.set_path(std::path::PathBuf::from(Self::CONTAINER_ROOT));

                rootfs.set_readonly(Some(false));
                spec.set_root(Some(rootfs));

                std::fs::write(
                    &config_path,
                    serde_json::to_string_pretty(&spec).map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to serialize Oci spec: {e}"),
                        )
                    })?,
                )
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to write Oci spec to {config_path:?}: {e}"),
                    )
                })?;
            }
        }
        Ok(())
    }

    pub fn get_container_path(&self, content_id: impl AsRef<Path>) -> impl AsRef<Path> {
        let container_path = self.containers.as_ref().join(content_id);
        container_path
    }

    pub fn get_base_path(&self, base_image: &BaseImage) -> impl AsRef<Path> {
        base_image.path()
    }

    pub fn container_root_path(&self, container_path: impl AsRef<Path>) -> impl AsRef<Path> {
        let container_root_path = container_path.as_ref().join(Self::CONTAINER_ROOT);
        container_root_path
    }

    pub fn get_payload_path(&self, content_id: impl AsRef<Path>) -> impl AsRef<Path> {
        let payload_path = self.payload_path.as_ref().join(content_id);
        payload_path
    }

    pub fn container_bin_path(&self, container_path: impl AsRef<Path>) -> impl AsRef<Path> {
        let container_root_path = container_path.as_ref().join(Self::CONTAINER_ROOT);
        container_root_path.join(Self::CONTAINER_BIN)
    }

    pub fn get_program_schema(
        &self,
        content_id: impl AsRef<Path>,
    ) -> std::io::Result<ProgramSchema> {
        tracing::info!("ContentId: {:?}", content_id.as_ref().to_string_lossy());
        let payload_path = self.get_payload_path(&content_id);
        let schema_path = self
            .get_schema_path(payload_path)
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::Other,
                "unable to find schema".to_string(),
            ))?;
        let mut str: String = String::new();
        let _file = std::fs::OpenOptions::new()
            .read(true)
            .write(false)
            .append(false)
            .truncate(false)
            .create(false)
            .open(&schema_path)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to open {schema_path}: {e}"),
                )
            })?
            .read_to_string(&mut str)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to read program schema from payload file: {e:?}"),
                )
            })?;

        let schema = toml::from_str(&str).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Error: compute.rs: 328: unable to parse schema file {e}"),
            )
        })?;

        Ok(schema)
    }

    fn get_schema_path(&self, payload_path: impl AsRef<Path>) -> Option<String> {
        tracing::info!(
            "search for entries in {:?}",
            payload_path.as_ref().canonicalize()
        );
        if let Ok(entries) = std::fs::read_dir(payload_path.as_ref()) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file()
                    && path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .starts_with("schema")
                {
                    return Some(path.to_string_lossy().into_owned());
                }
            }
        }

        None
    }
}

async fn copy_dir(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    let options = fs_extra::dir::CopyOptions::default();

    fs_extra::dir::copy(&src, &dst, &options).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to copy src dir to dst: {e:?}"),
        )
    })?;

    Ok(())
}

async fn link_dir(
    src: impl AsRef<Path> + std::fmt::Debug,
    dst: impl AsRef<Path> + std::fmt::Debug,
) -> std::io::Result<()> {
    tracing::info!("src: {}", src.as_ref().display());
    tracing::info!("dst: {}", src.as_ref().display());
    let link_path = src.as_ref().canonicalize().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Error canonicalizing path {src:?}: {e}"),
        )
    })?;
    tracing::info!("canonicalized src path: {}", &link_path.display());
    std::os::unix::fs::symlink(&link_path, &dst).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to symlink {dst:?} to {link_path:?}: {e:?}"),
        )
    })?;

    Ok(())
}

pub fn ensure_dir_exists(
    path: impl AsRef<std::path::Path> + std::fmt::Debug,
) -> std::io::Result<()> {
    if !path.as_ref().exists() {
        std::fs::create_dir_all(path.as_ref()).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Error creating dir path {path:?}: {e:?}"),
            )
        })?;
    }
    Ok(())
}
