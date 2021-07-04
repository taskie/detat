#[macro_use]
extern crate log;

use chardet::{charset2encoding, detect};
use encoding::{label::encoding_from_whatwg_label, DecoderTrap};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    error, fmt,
    fs::File,
    io,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::exit,
    str::FromStr,
};
use structopt::{clap, StructOpt};

#[derive(Debug)]
pub struct DetatError {
    kind: DetatErrorKind,
}

impl DetatError {
    pub fn invalid_opt(message: String) -> DetatError {
        DetatError { kind: DetatErrorKind::InvalidOpt(message) }
    }

    pub fn invalid_input(kind: InvalidInputErrorKind, message: String) -> DetatError {
        DetatError { kind: DetatErrorKind::InvalidInput(kind, message) }
    }

    pub fn decode(s: Cow<'static, str>) -> DetatError {
        DetatError { kind: DetatErrorKind::Decode(s) }
    }
}

#[derive(Debug)]
pub enum DetatErrorKind {
    Io(io::Error),
    InvalidOpt(String),
    InvalidInput(InvalidInputErrorKind, String),
    Decode(Cow<'static, str>),
    #[doc(hidden)]
    __Nonexhaustive,
}

#[derive(Debug)]
pub enum InvalidInputErrorKind {
    IsBinary,
    NoEncoding(String, String),
    LowConfidence(String, f32, f32),
    #[doc(hidden)]
    __Nonexhaustive,
}

impl error::Error for DetatError {
    fn description(&self) -> &str {
        "detat error"
    }
}

impl fmt::Display for DetatError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            DetatErrorKind::Io(ref e) => e.fmt(f),
            DetatErrorKind::InvalidInput(_, ref m) => f.write_str(m),
            DetatErrorKind::Decode(ref s) => f.write_str(s),
            _ => f.write_str("internal error"),
        }
    }
}

impl From<io::Error> for DetatError {
    fn from(ioerr: io::Error) -> DetatError {
        DetatError { kind: DetatErrorKind::Io(ioerr) }
    }
}
struct MyDecoderTrap(DecoderTrap);

impl FromStr for MyDecoderTrap {
    type Err = DetatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "strict" => Ok(MyDecoderTrap(DecoderTrap::Strict)),
            "replace" => Ok(MyDecoderTrap(DecoderTrap::Replace)),
            "ignore" => Ok(MyDecoderTrap(DecoderTrap::Ignore)),
            _ => Err(DetatError::invalid_opt(format!("invalid decoder trap: {}", s))),
        }
    }
}

impl std::fmt::Debug for MyDecoderTrap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            DecoderTrap::Strict => f.write_str("strict"),
            DecoderTrap::Replace => f.write_str("replace"),
            DecoderTrap::Ignore => f.write_str("ignore"),
            DecoderTrap::Call(_) => f.write_str("call"),
        }
    }
}

type DetatResult<T> = Result<T, DetatError>;

#[derive(Debug, StructOpt)]
#[structopt(name = "detat", about = "cat with chardet")]
#[structopt(long_version(option_env!("LONG_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))))]
#[structopt(setting(clap::AppSettings::ColoredHelp))]
pub struct Opt {
    #[structopt(name = "PATH", help = "An input file")]
    paths: Vec<PathBuf>,

    #[structopt(
        short,
        long,
        name = "CONFIDENCE_MIN",
        default_value = "0",
        help = "Fail if detected confidence is less than this"
    )]
    confidence_min: f32,

    #[structopt(
        short,
        long = "fallback",
        name = "ENCODING",
        help = "Use this encoding if detected confidence is less than <CONFIDENCE_MIN>"
    )]
    fallback_encoding: Option<String>,

    #[structopt(short, long, help = "Show results in a JSON Lines format")]
    json: bool,

    #[structopt(short, long, help = "Show statistics")]
    stat: bool,

    #[structopt(short = "b", long, help = "Print a binary input as it is")]
    allow_binary: bool,

    #[structopt(
        short = "t",
        long,
        name = "TRAP",
        default_value = "strict",
        help = "Use this trap handler if errors occur"
    )]
    decoder_trap: MyDecoderTrap,
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
    content: Option<String>,
}

pub struct Detat {
    confidence_min: f32,
    fallback_encoding: Option<String>,
    json: bool,
    stat: bool,
    allow_binary: bool,
    decoder_trap: DecoderTrap,
}

impl Detat {
    pub fn copy<R: Read, W: Write>(&self, r: &mut R, path: Option<&Path>, w: &mut W) -> DetatResult<Metadata> {
        let mut bs = Vec::new();
        let read_bytes = r.read_to_end(&mut bs)?;
        let chardet = ChardetResult::from_tuple(detect(bs.as_slice()));
        info!("predicted: {}, confidence: {}, language: {}", chardet.charset, chardet.confidence, chardet.language);
        if bs.is_empty() {
            let metadata = Metadata::default();
            if self.stat {
                if !self.json {
                    self.print_metadata(&metadata, path, w)?;
                }
            }
            return Ok(metadata);
        }
        let mut fallbacked = false;
        let charset = chardet.charset.clone();
        if charset.is_empty() {
            return if self.allow_binary {
                let metadata = Metadata { chardet, read_bytes, ..Metadata::default() };
                if self.stat {
                    if !self.json {
                        self.print_metadata(&metadata, path, w)?;
                    }
                } else {
                    w.write(&bs)?;
                }
                Ok(metadata)
            } else {
                Err(DetatError::invalid_input(InvalidInputErrorKind::IsBinary, "Input is binary".to_string()))
            };
        }
        let encoding = if chardet.confidence >= self.confidence_min {
            charset2encoding(&charset)
        } else {
            if let Some(enc) = &self.fallback_encoding {
                fallbacked = true;
                enc.as_str()
            } else {
                charset2encoding(&charset)
            }
        };
        let metadata = Metadata { chardet, encoding: encoding.to_string(), fallbacked, read_bytes };
        if self.stat {
            if !self.json {
                self.print_metadata(&metadata, path, w)?;
            }
            return Ok(metadata);
        }
        let enc = match encoding_from_whatwg_label(encoding) {
            Some(e) => e,
            None => {
                return Err(DetatError::invalid_input(
                    InvalidInputErrorKind::NoEncoding(encoding.to_string(), charset.clone()),
                    format!("no encoding: \"{}\" (charset: \"{}\")", encoding, charset),
                ));
            }
        };
        let s = match enc.decode(bs.as_slice(), self.decoder_trap) {
            Ok(s) => s,
            Err(e) => {
                return Err(DetatError::decode(e));
            }
        };
        w.write(s.as_bytes())?;
        Ok(metadata)
    }

    pub fn print_metadata<W: Write>(
        &self,
        metadata: &Metadata,
        path: Option<&Path>,
        w: &mut W,
    ) -> Result<(), io::Error> {
        write!(w, "---\n")?;
        write!(w, "Path: {}\n", path.and_then(|p| p.to_str()).unwrap_or("-"))?;
        write!(w, "Charset: {}\n", metadata.chardet.charset)?;
        write!(w, "Confidence: {}\n", metadata.chardet.confidence)?;
        write!(w, "Language: {}\n", metadata.chardet.language)?;
        Ok(())
    }

    pub fn copy_as_json<R: Read, W: Write>(&self, r: &mut R, path: Option<&Path>, w: &mut W) -> DetatResult<Metadata> {
        let mut content: Vec<u8> = Vec::new();
        let metadata = self.copy(r, path, &mut content)?;
        let mut json = {
            let path = path.and_then(|p| p.to_str()).map(|s| s.to_owned());
            let content = if self.stat || metadata.chardet.charset.is_empty() {
                None
            } else {
                Some(String::from_utf8(content).unwrap())
            };
            let output = Output { metadata: metadata.clone(), path, content };
            serde_json::to_vec(&output).unwrap()
        };
        json.push('\n' as u8);
        w.write_all(json.as_slice())?;
        Ok(metadata)
    }

    pub fn copy_from_stdin<W: Write>(&self, w: &mut W) -> DetatResult<Metadata> {
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        if self.json {
            self.copy_as_json(&mut handle, None, w)
        } else {
            self.copy(&mut handle, None, w)
        }
    }

    pub fn copy_from_file<W: Write>(&self, path: &Path, w: &mut W) -> DetatResult<Metadata> {
        let mut file = File::open(path)?;
        if self.json {
            self.copy_as_json(&mut file, Some(path), w)
        } else {
            self.copy(&mut file, Some(path), w)
        }
    }

    pub fn run(&self, path: &Path) -> DetatResult<Metadata> {
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
            return Err(DetatError::invalid_input(
                InvalidInputErrorKind::LowConfidence(metadata.chardet.charset.clone(), confidence, self.confidence_min),
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
    let detat = Detat {
        confidence_min: opt.confidence_min,
        fallback_encoding: opt.fallback_encoding,
        json: opt.json,
        stat: opt.stat,
        allow_binary: opt.allow_binary,
        decoder_trap: opt.decoder_trap.0,
    };
    let mut paths = opt.paths.clone();
    if paths.is_empty() {
        paths.push(PathBuf::from(""))
    }
    let mut error = false;
    for path in paths.iter() {
        let result = detat.run(path.as_ref());
        match result {
            Ok(_) => {}
            Err(e) => {
                error!("{}", e);
                error = true;
            }
        }
    }
    if error {
        exit(1)
    }
}
