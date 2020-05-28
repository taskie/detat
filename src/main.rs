#[macro_use]
extern crate log;

use std::{
    fs::File,
    io,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::exit,
};

use chardet::{charset2encoding, detect};
use encoding::{label::encoding_from_whatwg_label, DecoderTrap};
use serde::{Deserialize, Serialize};
use structopt::{clap, StructOpt};

#[derive(Debug, StructOpt)]
#[structopt(name = "detat")]
#[structopt(long_version(option_env!("LONG_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))))]
#[structopt(setting(clap::AppSettings::ColoredHelp))]
pub struct Opt {
    #[structopt(name = "PATHS")]
    paths: Vec<PathBuf>,

    #[structopt(short, long, default_value = "0")]
    confidence_min: f32,

    #[structopt(short, long = "fallback")]
    fallback_encoding: Option<String>,

    #[structopt(short, long)]
    json: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChardetResult {
    charset: String,
    confidence: f32,
    language: String,
}

impl ChardetResult {
    pub fn new(charset: String, confidence: f32, language: String) -> ChardetResult {
        ChardetResult { charset, confidence, language }
    }

    pub fn from_tuple((charset, confidence, language): (String, f32, String)) -> ChardetResult {
        Self::new(charset, confidence, language)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Metadata {
    chardet: ChardetResult,
    encoding: String,
    fallbacked: bool,
    read_bytes: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Output {
    path: Option<String>,
    metadata: Metadata,
    content: String,
}

pub struct Detat {
    confidence_min: f32,
    fallback_encoding: Option<String>,
    json: bool,
}

impl Detat {
    pub fn copy<R: Read, W: Write>(&self, r: &mut R, w: &mut W) -> Result<Metadata, io::Error> {
        let mut bs = Vec::new();
        let read_bytes = r.read_to_end(&mut bs)?;
        let chardet = ChardetResult::from_tuple(detect(bs.as_slice()));
        info!("predicted: {}, confidence: {}, language: {}", chardet.charset, chardet.confidence, chardet.language);
        if bs.is_empty() {
            return Ok(Metadata::default());
        }
        let mut fallbacked = false;
        let charset = chardet.charset.clone();
        let encoding = if !charset.is_empty() && chardet.confidence >= self.confidence_min {
            charset2encoding(&charset)
        } else {
            if let Some(enc) = &self.fallback_encoding {
                fallbacked = true;
                enc.as_str()
            } else {
                charset2encoding(&charset)
            }
        };
        let enc = match encoding_from_whatwg_label(encoding) {
            Some(e) => e,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("no encoding: \"{}\" (charset: \"{}\")", encoding, charset),
                ));
            }
        };
        let s = match enc.decode(bs.as_slice(), DecoderTrap::Strict) {
            Ok(s) => s,
            Err(e) => {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, e));
            }
        };
        w.write(s.as_bytes())?;
        Ok(Metadata { chardet, encoding: encoding.to_string(), fallbacked, read_bytes })
    }

    pub fn copy_as_json<R: Read, W: Write>(
        &self,
        r: &mut R,
        path: Option<&Path>,
        w: &mut W,
    ) -> Result<Metadata, io::Error> {
        let mut content: Vec<u8> = Vec::new();
        let metadata = self.copy(r, &mut content)?;
        let mut json = {
            let path = path.and_then(|p| p.to_str()).map(|s| s.to_owned());
            let output = Output { metadata: metadata.clone(), path, content: String::from_utf8(content).unwrap() };
            serde_json::to_vec(&output).unwrap()
        };
        json.push('\n' as u8);
        w.write_all(json.as_slice())?;
        Ok(metadata)
    }

    pub fn copy_from_stdin<W: Write>(&self, w: &mut W) -> Result<Metadata, io::Error> {
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        if self.json {
            self.copy_as_json(&mut handle, None, w)
        } else {
            self.copy(&mut handle, w)
        }
    }

    pub fn copy_from_file<W: Write>(&self, path: &Path, w: &mut W) -> Result<Metadata, io::Error> {
        let mut file = File::open(path)?;
        if self.json {
            self.copy_as_json(&mut file, Some(path), w)
        } else {
            self.copy(&mut file, w)
        }
    }

    pub fn run(&self, path: &Path) -> Result<Metadata, io::Error> {
        let stdout = io::stdout();
        let w = stdout.lock();
        let mut bw = BufWriter::new(w);
        let path_str = path.to_str().unwrap();
        let metadata = if path_str.is_empty() || path_str == "-" {
            self.copy_from_stdin(&mut bw)
        } else {
            self.copy_from_file(&path, &mut bw)
        }?;
        let confidence = metadata.chardet.confidence;
        if metadata.read_bytes > 0 && !metadata.fallbacked && confidence < self.confidence_min {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "confidence: {} < {} (predicted: {})",
                    confidence, self.confidence_min, metadata.chardet.charset
                ),
            ));
        }
        Ok(metadata)
    }
}

fn main() {
    env_logger::init();
    let opt = Opt::from_args();
    let detat = Detat { confidence_min: opt.confidence_min, fallback_encoding: opt.fallback_encoding, json: opt.json };
    let mut paths = opt.paths.clone();
    if paths.is_empty() {
        paths.push(PathBuf::from(""))
    }
    for path in paths.iter() {
        let result = detat.run(path.as_ref());
        match result {
            Ok(_) => {}
            Err(e) => {
                error!("{}", e);
                exit(1);
            }
        }
    }
}
