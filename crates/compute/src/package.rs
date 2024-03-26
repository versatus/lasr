use derive_builder::Builder;
use secp256k1::{Message, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::{collections::BTreeMap, fmt::Display, path::PathBuf};

use lasr_types::RecoverableSignature;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename = "camelCase")]
pub struct LasrObjectCid(String);

impl From<String> for LasrObjectCid {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LasrObjectRuntime {
    None,
    Bin,
    Wasm,
    Node,
    Bun,
    Python,
    Java,
    Other(String),
}

impl From<&str> for LasrObjectRuntime {
    fn from(value: &str) -> Self {
        match value {
            "bin" => LasrObjectRuntime::Bin,
            "wasm" => LasrObjectRuntime::Wasm,
            "node" => LasrObjectRuntime::Node,
            "bun" => LasrObjectRuntime::Bun,
            "python" => LasrObjectRuntime::Python,
            "java" => LasrObjectRuntime::Java,
            _ => LasrObjectRuntime::None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocumentFormat {
    Pdf,
    Word,
    Excel,
    Powerpoint,
    Zip,
    Rar,
    SevenZ,
    Tar,
    Gzip,
    TarGzip,
}

impl Display for DocumentFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pdf => write!(f, "pdf"),
            Self::Word => write!(f, "doc"),
            Self::Excel => write!(f, "xlsx"),
            Self::Powerpoint => write!(f, "ppt"),
            Self::Zip => write!(f, "zip"),
            Self::Rar => write!(f, "rar"),
            Self::SevenZ => write!(f, "7z"),
            Self::Tar => write!(f, "tar"),
            Self::Gzip => write!(f, "gz"),
            Self::TarGzip => write!(f, "tar.gz"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileFormat {
    PlainText,
    Html,
    Xml,
    Json,
    Toml,
    Yaml,
    Csv,
    Markdown,
    Sql,
    Cad,
    Stl,
    Blender,
    Obj,
    Fbx,
    Collada,
    Ply,
    ThreeDS,
    Gltf,
    Glb,
    Document(DocumentFormat),
}

impl Display for FileFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileFormat::Document(document_format) => write!(f, "{}", document_format),
            FileFormat::PlainText => write!(f, "txt"),
            FileFormat::Html => write!(f, "html"),
            FileFormat::Xml => write!(f, "xml"),
            FileFormat::Json => write!(f, "json"),
            FileFormat::Toml => write!(f, "toml"),
            FileFormat::Yaml => write!(f, "yaml"),
            FileFormat::Csv => write!(f, "csv"),
            FileFormat::Markdown => write!(f, "md"),
            FileFormat::Sql => write!(f, "sql"),
            FileFormat::Cad => write!(f, "dwg"),
            FileFormat::Stl => write!(f, "stl"),
            FileFormat::Blender => write!(f, "blend"),
            FileFormat::Obj => write!(f, "obj"),
            FileFormat::Fbx => write!(f, "fbx"),
            FileFormat::Collada => write!(f, "dae"),
            FileFormat::Ply => write!(f, "ply"),
            FileFormat::ThreeDS => write!(f, "3ds"),
            FileFormat::Gltf => write!(f, ".gltf"),
            FileFormat::Glb => write!(f, ".glb"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageFormat {
    Jpeg,
    Png,
    Gif,
    Bmp,
    Svg,
    Other(String),
}

impl Display for ImageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Jpeg => write!(f, "jpeg"),
            Self::Png => write!(f, "png"),
            Self::Gif => write!(f, "gif"),
            Self::Bmp => write!(f, "bmp"),
            Self::Svg => write!(f, "svg"),
            Self::Other(ext) => write!(f, "{}", ext),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioFormat {
    Mp3,
    Wav,
    Aac,
    Flac,
    Other(String),
}

impl Display for AudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mp3 => write!(f, "mp3"),
            Self::Wav => write!(f, "wav"),
            Self::Aac => write!(f, "aac"),
            Self::Flac => write!(f, "flac"),
            Self::Other(ext) => write!(f, "{}", ext),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoFormat {
    Mp4,
    Avi,
    Mov,
    Wmv,
}

impl Display for VideoFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mp4 => write!(f, "mp4"),
            Self::Avi => write!(f, "avi"),
            Self::Mov => write!(f, "mov"),
            Self::Wmv => write!(f, "wmv"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProgramFormat {
    Executable,
    Script(String),
    Lib(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LasrContentType {
    Document(FileFormat),
    Image(ImageFormat),
    Audio(AudioFormat),
    Video(VideoFormat),
    Program(ProgramFormat),
}

impl From<&str> for LasrContentType {
    fn from(value: &str) -> Self {
        match value {
            "txt" => LasrContentType::Document(FileFormat::PlainText),
            "html" => LasrContentType::Document(FileFormat::Html),
            "xml" => LasrContentType::Document(FileFormat::Xml),
            "json" => LasrContentType::Document(FileFormat::Json),
            "csv" => LasrContentType::Document(FileFormat::Csv),
            "md" => LasrContentType::Document(FileFormat::Markdown),
            "sql" => LasrContentType::Document(FileFormat::Sql),
            "dwg" => LasrContentType::Document(FileFormat::Cad),
            "stl" => LasrContentType::Document(FileFormat::Stl),
            "blend" => LasrContentType::Document(FileFormat::Blender),
            "obj" => LasrContentType::Document(FileFormat::Obj),
            "fbx" => LasrContentType::Document(FileFormat::Fbx),
            "dae" => LasrContentType::Document(FileFormat::Collada),
            "ply" => LasrContentType::Document(FileFormat::Ply),
            "3ds" => LasrContentType::Document(FileFormat::ThreeDS),
            "gltf" => LasrContentType::Document(FileFormat::Gltf),
            "glb" => LasrContentType::Document(FileFormat::Glb),
            "jpeg" | "jpg" => LasrContentType::Image(ImageFormat::Jpeg),
            "png" => LasrContentType::Image(ImageFormat::Png),
            "gif" => LasrContentType::Image(ImageFormat::Gif),
            "bmp" => LasrContentType::Image(ImageFormat::Bmp),
            "svg" => LasrContentType::Image(ImageFormat::Svg),
            "mp3" => LasrContentType::Audio(AudioFormat::Mp3),
            "wav" => LasrContentType::Audio(AudioFormat::Wav),
            "aac" => LasrContentType::Audio(AudioFormat::Aac),
            "flac" => LasrContentType::Audio(AudioFormat::Flac),
            "mp4" => LasrContentType::Video(VideoFormat::Mp4),
            "avi" => LasrContentType::Video(VideoFormat::Avi),
            "mov" => LasrContentType::Video(VideoFormat::Mov),
            "wmv" => LasrContentType::Video(VideoFormat::Wmv),
            "" => LasrContentType::Program(ProgramFormat::Executable),
            _ => LasrContentType::Program(ProgramFormat::Script(value.to_string())),
        }
    }
}

impl From<PathBuf> for LasrContentType {
    fn from(value: PathBuf) -> Self {
        let extension = value
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_lowercase();

        match extension.as_str() {
            "txt" => LasrContentType::Document(FileFormat::PlainText),
            "html" => LasrContentType::Document(FileFormat::Html),
            "xml" => LasrContentType::Document(FileFormat::Xml),
            "json" => LasrContentType::Document(FileFormat::Json),
            "csv" => LasrContentType::Document(FileFormat::Csv),
            "md" => LasrContentType::Document(FileFormat::Markdown),
            "sql" => LasrContentType::Document(FileFormat::Sql),
            "dwg" => LasrContentType::Document(FileFormat::Cad),
            "stl" => LasrContentType::Document(FileFormat::Stl),
            "blend" => LasrContentType::Document(FileFormat::Blender),
            "obj" => LasrContentType::Document(FileFormat::Obj),
            "fbx" => LasrContentType::Document(FileFormat::Fbx),
            "dae" => LasrContentType::Document(FileFormat::Collada),
            "ply" => LasrContentType::Document(FileFormat::Ply),
            "3ds" => LasrContentType::Document(FileFormat::ThreeDS),
            "gltf" => LasrContentType::Document(FileFormat::Gltf),
            "glb" => LasrContentType::Document(FileFormat::Glb),
            "jpeg" | "jpg" => LasrContentType::Image(ImageFormat::Jpeg),
            "png" => LasrContentType::Image(ImageFormat::Png),
            "gif" => LasrContentType::Image(ImageFormat::Gif),
            "bmp" => LasrContentType::Image(ImageFormat::Bmp),
            "svg" => LasrContentType::Image(ImageFormat::Svg),
            "mp3" => LasrContentType::Audio(AudioFormat::Mp3),
            "wav" => LasrContentType::Audio(AudioFormat::Wav),
            "aac" => LasrContentType::Audio(AudioFormat::Aac),
            "flac" => LasrContentType::Audio(AudioFormat::Flac),
            "mp4" => LasrContentType::Video(VideoFormat::Mp4),
            "avi" => LasrContentType::Video(VideoFormat::Avi),
            "mov" => LasrContentType::Video(VideoFormat::Mov),
            "wmv" => LasrContentType::Video(VideoFormat::Wmv),
            "" => LasrContentType::Program(ProgramFormat::Executable),
            _ => LasrContentType::Program(ProgramFormat::Script(extension)),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LasrPackageType {
    Runtime(LasrObjectRuntime),
    Program(LasrObjectRuntime),
    Content(LasrContentType),
}

impl From<(&str, &str)> for LasrPackageType {
    fn from(value: (&str, &str)) -> Self {
        match value.0 {
            "runtime" => LasrPackageType::Runtime(LasrObjectRuntime::from(value.1)),
            "content" => LasrPackageType::Content(LasrContentType::from(value.1)),
            _ => LasrPackageType::Program(LasrObjectRuntime::from(value.1)),
        }
    }
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LasrObjectPayload {
    pub object_content_type: LasrContentType,
    pub object_path: String,
    pub object_cid: LasrObjectCid,
    #[builder(default = "BTreeMap::new()")]
    pub object_annotations: BTreeMap<String, String>,
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LasrObject {
    pub object_payload: LasrObjectPayload,
    pub object_sig: RecoverableSignature,
}

impl LasrObject {
    pub fn object_cid(&self) -> &String {
        &self.object_payload.object_cid.0
    }

    pub fn object_content_type(&self) -> &LasrContentType {
        &self.object_payload.object_content_type
    }

    pub fn object_path(&self) -> &String {
        &self.object_payload.object_path
    }

    pub fn object_annotations(&self) -> &BTreeMap<String, String> {
        &self.object_payload.object_annotations
    }
}

impl SignableObject for LasrObjectPayload {}

#[derive(Builder, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LasrPackagePayload {
    pub api_version: u32,
    pub package_version: u32,
    pub package_name: String,
    pub package_author: String,
    pub package_type: LasrPackageType,
    pub package_objects: Vec<LasrObject>,
    pub package_replaces: Vec<LasrObjectCid>,
    pub package_entrypoint: String,
    pub package_program_args: Vec<String>,
    pub package_annotations: BTreeMap<String, String>,
}

impl SignableObject for LasrPackagePayload {}

#[derive(Builder, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LasrPackage {
    pub package_payload: LasrPackagePayload,
    pub package_sig: RecoverableSignature,
}

pub trait SignableObject: Serialize {
    fn hash(&self) -> Result<Vec<u8>, Box<bincode::ErrorKind>> {
        let mut hasher = Sha3_256::new();
        hasher.update(bincode::serialize(&self)?);
        Ok(hasher.finalize().to_vec())
    }

    fn sign(&self, sk: &SecretKey) -> Result<RecoverableSignature, std::io::Error> {
        let ctx = Secp256k1::new();

        let bytes = self
            .hash()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        let msg = Message::from_digest_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        Ok(ctx.sign_ecdsa_recoverable(&msg, sk).into())
    }
}
