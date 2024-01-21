use std::{ffi::OsStr, fmt::Display};
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use oci_spec::runtime::{ProcessBuilder, RootBuilder, Spec};
use std::process::Stdio;
use std::io::Read;
use crate::{Inputs, Outputs, ProgramSchema};

#[allow(unused)]
use ipfs_api::{IpfsApi, IpfsClient};

pub enum BaseImage {
    Wasm,
    Bin,
    Python,
    Node,
    Java
}

impl Display for BaseImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BaseImage::Bin => write!(f, "{}", "bin"),
            BaseImage::Wasm => write!(f, "{}", "wasm"),
            BaseImage::Python => write!(f, "{}", "python"),
            BaseImage::Node => write!(f, "{}", "node"),
            BaseImage::Java => write!(f, "{}", "java")
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

#[allow(unused)]
pub struct IpfsManager {
    client: IpfsClient
}

#[derive(Debug, Clone)]
pub struct OciManager {
    bundler: OciBundler<String, String>,
}

impl Display for OciManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl OciManager {
    pub fn new(
        bundler: OciBundler<String, String>,
    ) -> Self {
        Self {
            bundler,
        }
    }

    pub async fn bundle(
        &self,
        content_id: impl AsRef<Path>,
        base_image: BaseImage
    ) -> Result<(), std::io::Error> {
        self.bundler.bundle(content_id, base_image).await
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
        content_id: impl AsRef<Path> + Send, 
        inputs: Inputs,
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
            let mut child = Command::new("sudo")
                .arg("runsc")
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

            //.map_err(|e| {
            //    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
            //})?;
            log::info!("result from container: {container_id} = {:#?}", res);

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
        base_image: BaseImage
    ) -> Result<(), std::io::Error> {
        let base_path = self.get_base_path(base_image);
        let container_path = self.get_container_path(&content_id);
        if !container_path.as_ref().exists() {
            std::fs::create_dir_all(container_path.as_ref())?;
        }
        let container_root_path = self.container_root_path(&container_path);
        if !container_root_path.as_ref().exists() {
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
        
        let mut args = vec![format!("/{}/{}", content_id.as_ref().display(), entrypoint.to_string())];
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

    pub fn get_base_path(&self, base_image: BaseImage) -> impl AsRef<Path> {
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
        let payload_path = self.get_payload_path(content_id);
        let schema_path = self.get_schema_path(payload_path).ok_or(
            std::io::Error::new(std::io::ErrorKind::Other, "unable to find schema".to_string())
        )?;
        let mut str: String = String::new();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(false)
            .append(false)
            .truncate(false)
            .create(false)
            .open(schema_path)?.read_to_string(&mut str)?;
        
        let schema = toml::from_str(&str).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, "unable to parse schema file")
        })?;

        Ok(schema)
    }

    fn get_schema_path(&self, payload_path: impl AsRef<Path>) -> Option<String> {
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
