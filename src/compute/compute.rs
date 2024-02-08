use std::{ffi::OsStr, fmt::Display};
use std::path::Path;
use ractor::ActorRef;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use oci_spec::runtime::{ProcessBuilder, RootBuilder, Spec};
use web3_pkg::web3_store::Web3Store;
use std::process::Stdio;
use std::io::Read;
use std::io::Write;
use crate::{Inputs, ProgramSchema, ExecutorMessage, ActorType, Transaction, LasrPackage, LasrContentType, ProgramFormat, LasrPackageType, LasrObjectRuntime};

#[allow(unused)]
use ipfs_api::{IpfsApi, IpfsClient};

#[derive(Debug)]
pub enum BaseImage {
    Wasm,
    Bin,
    Python,
    Node,
    Bun,
    Java
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
            BaseImage::Bin => write!(f, "{}", "bin"),
            BaseImage::Wasm => write!(f, "{}", "wasm"),
            BaseImage::Python => write!(f, "{}", "python"),
            BaseImage::Node => write!(f, "{}", "nodejs"),
            BaseImage::Java => write!(f, "{}", "java"),
            BaseImage::Bun => write!(f, "{}", "bunjs")
        }
    }
}

impl BaseImage {
    pub fn path(&self) -> String {
        match self {
            _ => format!("./base_image/{}", self.to_string())
        }
    }
}

#[derive(Debug)]
pub struct PackageContainerMetadata {
    base_image: BaseImage,
    cid: String,
    entrypoint: String, 
    program_args: Vec<String>
}

impl PackageContainerMetadata {
    pub fn new(base_image: BaseImage, cid: String, entrypoint: String, program_args: Vec<String>) -> Self {
        Self { base_image, cid, entrypoint, program_args } 
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

pub struct OciManager {
    bundler: OciBundler<String, String>,
    store: Web3Store,
}

impl OciManager {
    pub fn new(
        bundler: OciBundler<String, String>,
        store: Web3Store,
    ) -> Self {
        Self {
            bundler,
            store
        }
    }

    pub async fn pin_object(&self, content_id: &str, recursive: bool) -> Result<(), std::io::Error> {
        let cids = self.store.pin_object(content_id, recursive).await.map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string()
            )
        })?;

        log::info!("Pinned object: {:?}", cids);
        Ok(())
    }

    pub async fn create_payload_package(&self, content_id: impl AsRef<Path>) -> Result<Option<PackageContainerMetadata>, std::io::Error> {
        let cid = content_id.as_ref().to_string_lossy().to_string();
        let payload_path_string = self.bundler.get_payload_path(
            content_id.as_ref()
        ).as_ref()
            .to_string_lossy()
            .to_string();

        log::info!("Attempting to read DAG for {} from Web3Store...", &cid);
        let package_data = self.store.read_dag(
            &cid
        ).await.map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string()
            )
        })?;

        let package_dir = format!("{}/{}", &payload_path_string, &cid);

        log::info!("creating all directories in path: {}", &package_dir);
        std::fs::create_dir_all(&package_dir)?;

        let package: LasrPackage = serde_json::from_slice(&package_data)?;
        log::info!(
            "Package '{}' version {} from '{}' is type {:?}",
            &package.package_payload.package_name,
            &package.package_payload.package_version, 
            &package.package_payload.package_author,
            &package.package_payload.package_type,
        );

        let container_metadata = match package.package_payload.package_type {
            LasrPackageType::Program(runtime) => {
                Some(
                    PackageContainerMetadata::new(
                        BaseImage::from(runtime),
                        cid.to_string(),
                        package.package_payload.package_entrypoint,
                        package.package_payload.package_program_args,
                    )
                )
            }
            _ => None
        };

        let package_metadata_filepath = format!("{}/metadata.json", &package_dir);

        log::info!("creating package metadata file: {}", &package_metadata_filepath);
        let mut f = std::fs::File::create(&package_metadata_filepath)?;

        f.write_all(&package_data)?;

        let mut package_object_iter = package.package_payload.package_objects.into_iter();

        //TODO(asmith) convert into a parallel iterator
        while let Some(obj) = package_object_iter.next() {
            log::info!("getting object: {} from Web3Store", &obj.object_cid());
            let object_data = self.store.read_object(obj.object_cid()).await.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string()
                )
            })?;

            let mut object_path = obj.object_path().strip_prefix("./payload/").unwrap_or(obj.object_path());
            if object_path == obj.object_path() {
                object_path = obj.object_path().strip_prefix("./").unwrap_or(obj.object_path());
            }

            let object_filepath = match obj.object_content_type() {
                LasrContentType::Program(program_format) => {
                    match program_format {
                        ProgramFormat::Executable => {
                            format!(
                                "{}/{}", 
                                &package_dir, 
                                object_path,
                            )
                        }
                        ProgramFormat::Script(_) => {
                            format!(
                                "{}/{}", 
                                &package_dir, 
                                object_path,
                            )
                        }
                        ProgramFormat::Lib(_) => {
                            format!(
                                "{}/{}",
                                &package_dir,
                                object_path
                            )
                        }
                    }
                }
                LasrContentType::Document(_) => {
                    format!(
                        "{}/{}",
                        &package_dir,
                        object_path
                    )
                }
                LasrContentType::Image(_) => {
                    format!(
                        "{}/{}",
                        &package_dir,
                        object_path
                    )
                }
                LasrContentType::Audio(_) => {
                    format!(
                        "{}/{}",
                        &package_dir,
                        object_path
                    )
                }
                LasrContentType::Video(_) => {
                    format!(
                        "{}/{}",
                        &package_dir,
                        object_path
                    )
                }
            };

            log::info!("creating missing directories in: {}", &object_filepath);
            let object_path = Path::new(&object_filepath);
            if let Some(parent) = object_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            log::info!("writing object to: {}", &object_filepath);

            let mut f = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .truncate(true)
                .append(false)
                .create(true)
                .open(object_filepath)?;

            f.write_all(&object_data)?;
        }

        Ok(container_metadata)

    }

    pub async fn bundle(
        &self,
        content_id: impl AsRef<Path>,
    ) -> Result<(), std::io::Error> {
        let cid = content_id.as_ref().to_string_lossy().to_owned().to_string();
        log::info!("attempting to create bundle for {}", cid);
        let container_metadata = self.create_payload_package(content_id).await?;
        if let Some(metadata) = container_metadata {
            log::info!("received container metadata: {:?}", &metadata);
            log::info!("building container bundle");
            self.bundler.bundle(&cid, &metadata).await?;
            self.add_payload(&cid).await?;
            self.base_spec(&cid).await?;
            let program_args = {
                if metadata.program_args().is_empty() {
                    None
                } else {
                    Some(metadata.program_args().clone())
                }
            };
            self.customize_spec(cid, metadata.entrypoint(), program_args)?;
            return Ok(())
        }

        return Err(
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "BaseImage not found, unsuported runtime or content type"
            )
        )
    }


    pub async fn add_payload(
        &self,
        content_id: impl AsRef<Path>
    ) -> Result<(), std::io::Error> {
        self.bundler.add_payload(content_id).await
    }

    pub async fn base_spec(
        &self,
        content_id: impl AsRef<Path>
    ) -> Result<(), std::io::Error> {
        self.bundler.base_spec(content_id).await
    }

    pub fn customize_spec(
        &self,
        content_id: impl AsRef<Path>,
        entrypoint: &str,
        program_args: Option<Vec<String>>
    ) -> Result<(), std::io::Error> {
        self.bundler.customize_spec(content_id, entrypoint, program_args)
    }

    pub fn get_program_schema(
        &self,
        content_id: impl AsRef<Path>
    ) -> std::io::Result<ProgramSchema> {
        self.bundler.get_program_schema(content_id)
    }

    pub async fn run_container(
        &self,
        content_id: impl AsRef<Path> + Send + 'static, 
        transaction: Option<Transaction>,
        inputs: Inputs,
        transaction_hash: Option<String>,
    ) -> Result<tokio::task::JoinHandle<Result<String, std::io::Error>>, std::io::Error> {
        let container_path = self.bundler.get_container_path(&content_id)
            .as_ref()
            .to_string_lossy()
            .into_owned();

        let container_id = content_id.as_ref()
            .to_string_lossy()
            .into_owned();

        let inner_inputs = inputs.clone();
        Ok(tokio::spawn(async move {
            let mut child = Command::new("runsc")
                .arg("--rootless")
                .arg("--network=none")
                .arg("run")
                .arg("-bundle")
                .arg(&container_path)
                .arg(&container_id)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            
            let mut stdin = child.stdin.take().ok_or( {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("unable to acquire child stdin, compute.rs: 120")
                )
            })?;
            let stdio_inputs = serde_json::to_string(&inner_inputs.clone())?;
            log::info!("passing inputs to stdio: {:#?}", &stdio_inputs);
            let _ = tokio::task::spawn(async move {
                stdin.write_all(
                    stdio_inputs.clone().as_bytes()
                ).await?;

                Ok::<_, std::io::Error>(())
            }).await?;
            let output = child.wait_with_output().await?;
            let res: String = String::from_utf8_lossy(&output.stdout).into_owned();

            log::info!("result from container: {container_id} = {:#?}", res);

            let actor: ActorRef<ExecutorMessage> = ractor::registry::where_is(
                ActorType::Executor.to_string()
            ).ok_or(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "unable to acquire Executor actor from inside container execution thread"
                )
            )?.into();

            let message = ExecutorMessage::Results {
                content_id: content_id.as_ref().to_string_lossy().into_owned(), 
                transaction_hash,
                transaction,
            };

            actor.cast(message).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

            Ok::<_, std::io::Error>(res)
        }))
    }
}

#[derive(Builder, Clone, Debug)]
pub struct OciBundler<R: AsRef<OsStr>, P: AsRef<Path>> {
    containers: P,
    #[allow(unused)]
    base_images: P,
    runtime: R,
    payload_path: P
}

impl<R: AsRef<OsStr>, P: AsRef<Path>> OciBundler<R, P> {
    pub const CONTAINER_ROOT: &'static str = "rootfs";
    pub const CONTAINER_BIN: &'static str = "bin";

    pub fn new(runtime: R, containers: P, base_images: P, payload_path: P) -> Self {
        Self {
            containers,
            base_images,
            runtime,
            payload_path
        }
    }

    pub async fn bundle(
        &self,
        content_id: impl AsRef<Path>,
        container_metadata: &PackageContainerMetadata
    ) -> Result<(), std::io::Error> {
        let base_path = self.get_base_path(container_metadata.base_image());
        let container_path = self.get_container_path(&content_id);
        if !container_path.as_ref().exists() {
            log::info!("container path: {} doesn't exist, creating...", container_path.as_ref().to_string_lossy().to_string());
            std::fs::create_dir_all(container_path.as_ref())?;
        }
        let container_root_path = self.container_root_path(&container_path);
        if !container_root_path.as_ref().exists() {
            log::info!("container root path: {} doesn't exist, creating...", container_root_path.as_ref().to_string_lossy().to_string());
            link_dir(&base_path.as_ref().join(Self::CONTAINER_ROOT), &container_root_path.as_ref()).await?;
        }

        Ok(())
    }

    pub async fn add_payload(
        &self,
        content_id: impl AsRef<Path>
    ) -> Result<(), std::io::Error> {
        let container_path = self.get_container_path(&content_id);
        let container_root = self.container_root_path(&container_path);
        let payload_path = self.get_payload_path(&content_id);
        log::info!("Attempting to copy {:?} to {:?}", &payload_path.as_ref().canonicalize(), &container_root.as_ref().canonicalize());
        if let Err(e) = copy_dir(payload_path, container_root).await {
            log::error!("Error adding payload: {e}");
        };

        Ok(())
    }

    pub async fn base_spec(
        &self,
        content_id: impl AsRef<Path>
    ) -> Result<(), std::io::Error> {
        let container_path = self.get_container_path(&content_id);
        Command::new(&self.runtime)
            .arg("spec")
            .current_dir(container_path)
            .output().await?;

        Ok(())
    }

    pub fn customize_spec(
        &self,
        content_id: impl AsRef<Path>,
        entrypoint: &str,
        program_args: Option<Vec<String>>
    ) -> Result<(), std::io::Error> {
        let container_path = self.get_container_path(&content_id);
        let config_path = container_path.as_ref().join("config.json");

        let mut spec: Spec = Spec::load(&config_path).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e)
        })?;

        let mut proc = if let Some(gen_proc) = spec.process() {
            gen_proc.to_owned()
        } else {
            ProcessBuilder::default()
                .build()
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other, e
                    )
                })?
        };
        
        let mut args = vec![format!("/{}/{}/{}/", content_id.as_ref().display(), content_id.as_ref().display(), entrypoint)];
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
            RootBuilder::default()
                .build()
                .map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::Other, e)
                })?
        };

        rootfs.set_path(
            std::path::PathBuf::from(Self::CONTAINER_ROOT)
        );

        rootfs.set_readonly(Some(false));
        spec.set_root(Some(rootfs));

        std::fs::write(config_path, serde_json::to_string_pretty(&spec)?)?;
        Ok(())
    }

    pub fn get_container_path(
        &self,
        content_id: impl AsRef<Path>
    ) -> impl AsRef<Path> {
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
        let container_bin_path = container_root_path.join(Self::CONTAINER_BIN);
        container_bin_path
    }

    pub fn get_program_schema(&self, content_id: impl AsRef<Path>) -> std::io::Result<ProgramSchema> {
        log::info!("ContentId: {:?}", content_id.as_ref().to_string_lossy());
        let payload_path = self.get_payload_path(&content_id);
        let schema_path = self.get_schema_path(payload_path).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "unable to find schema".to_string())
        )?;
        let mut str: String = String::new();
        let _file = std::fs::OpenOptions::new()
            .read(true)
            .write(false)
            .append(false)
            .truncate(false)
            .create(false)
            .open(schema_path)?.read_to_string(&mut str)?;
        
        let schema = toml::from_str(&str).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("Error: compute.rs: 328: unable to parse schema file {e}"))
        })?;

        Ok(schema)
    }

    fn get_schema_path(&self, payload_path: impl AsRef<Path>) -> Option<String> {
        log::info!("search for entries in {:?}", payload_path.as_ref().canonicalize());
        if let Ok(entries) = std::fs::read_dir(payload_path.as_ref()) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    if path.file_name().unwrap_or_default().to_string_lossy().starts_with("schema") {
                        return Some(path.to_string_lossy().into_owned());
                    }
                }
            }
        }

        None
    }
}

async fn copy_dir(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path> 
) -> std::io::Result<()> {
    let options = fs_extra::dir::CopyOptions::default();
        
    fs_extra::dir::copy(&src, &dst, &options).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            e
        )
    })?;

    Ok(())
}

async fn link_dir(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
) -> std::io::Result<()> {
    let link_path = src.as_ref().canonicalize()?;
    std::os::unix::fs::symlink(link_path, dst)?;

    Ok(())
}

pub fn ensure_dir_exists(path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
    if !path.as_ref().exists() {
        std::fs::create_dir_all(path.as_ref())?;
    }
    Ok(())
}
