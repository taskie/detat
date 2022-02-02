#[macro_use]
extern crate log;

use chardetng::EncodingDetector;
use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    error, fmt,
    fs::File,
    io,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::exit,
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
#[non_exhaustive]
pub enum DetatErrorKind {
    Io(io::Error),
    InvalidOpt(String),
    InvalidInput(InvalidInputErrorKind, String),
    Decode(Cow<'static, str>),
}

#[derive(Debug)]
#[non_exhaustive]
pub enum InvalidInputErrorKind {
    IsBinary,
    NoEncoding(String),
    NoConfidence(String),
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
        long = "fallback",
        name = "ENCODING",
        help = "Use this encoding if detected confidence is less than <CONFIDENCE_MIN>"
    )]
    fallback_encoding: Option<String>,

    #[structopt(short, long, help = "Show results in a JSON Lines format")]
    json: bool,

    #[structopt(short, long, help = "Show statistics")]
    stat: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EncodingResult {
    name: String,
}

impl EncodingResult {
    pub fn with_name(name: &str) -> EncodingResult {
        EncodingResult {
            name: name.to_owned(),
        }
    }
    
    pub fn from_encoding(encoding: &'static Encoding) -> EncodingResult {
        EncodingResult {
            name: encoding.name().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChardetResult {
    encoding: EncodingResult,
    has_confidence: bool,
}

impl ChardetResult {
    pub fn new(encoding: EncodingResult, has_confidence: bool) -> ChardetResult {
        ChardetResult { encoding, has_confidence }
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
    fallback_encoding: Option<String>,
    json: bool,
    stat: bool,
}

impl Detat {
    pub fn copy<R: Read, W: Write>(&self, r: &mut R, path: Option<&Path>, w: &mut W) -> DetatResult<Metadata> {
        let mut bs = Vec::new();
        let read_bytes = r.read_to_end(&mut bs)?;
        let mut detector = EncodingDetector::new();
        detector.feed(&bs, true);
        let (encoding, has_confidence) = detector.guess_assess(None, true);
        info!("predicted: {}, has_confidence: {}", encoding.name(), has_confidence);
        if bs.is_empty() {
            let metadata = Metadata::default();
            if self.stat && !self.json {
                self.print_metadata(&metadata, path, w)?;
            }
            return Ok(metadata);
        }
        let mut fallbacked = false;
        let encoding = if has_confidence {
            EncodingResult::from_encoding(encoding)
        } else if let Some(enc) = &self.fallback_encoding {
            fallbacked = true;
            EncodingResult::with_name(enc)
        } else {
            EncodingResult::from_encoding(encoding)
        };
        let encoding_name = encoding.name.clone();
        let metadata = Metadata { chardet: ChardetResult::new(encoding, has_confidence), encoding: encoding_name.clone(), fallbacked, read_bytes };
        if self.stat {
            if !self.json {
                self.print_metadata(&metadata, path, w)?;
            }
            return Ok(metadata);
        }
        let enc = match Encoding::for_label(encoding_name.as_bytes()) {
            Some(e) => e,
            None => {
                return Err(DetatError::invalid_input(
                    InvalidInputErrorKind::NoEncoding(encoding_name.clone()),
                    format!("no encoding: \"{}\"", &encoding_name),
                ));
            }
        };
        let (s, _, _) = enc.decode(bs.as_slice());
        w.write_all(s.as_bytes())?;
        Ok(metadata)
    }

    pub fn print_metadata<W: Write>(
        &self,
        metadata: &Metadata,
        path: Option<&Path>,
        w: &mut W,
    ) -> Result<(), io::Error> {
        writeln!(w, "---")?;
        writeln!(w, "Path: {}", path.and_then(|p| p.to_str()).unwrap_or("-"))?;
        writeln!(w, "Charset: {}", metadata.chardet.encoding.name)?;
        writeln!(w, "Has confidence: {}", metadata.chardet.has_confidence)?;
        Ok(())
    }

    pub fn copy_as_json<R: Read, W: Write>(&self, r: &mut R, path: Option<&Path>, w: &mut W) -> DetatResult<Metadata> {
        let mut content: Vec<u8> = Vec::new();
        let metadata = self.copy(r, path, &mut content)?;
        let mut json = {
            let path = path.and_then(|p| p.to_str()).map(|s| s.to_owned());
            let content = Some(String::from_utf8(content).unwrap());
            let output = Output { metadata: metadata.clone(), path, content };
            serde_json::to_vec(&output).unwrap()
        };
        json.push(b'\n');
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
            self.copy_from_file(path, &mut bw)
        }?;
        if metadata.read_bytes > 0 && !metadata.fallbacked && ! metadata.chardet.has_confidence {
            let encoding_name = metadata.chardet.encoding.name.clone();
            return Err(DetatError::invalid_input(
                InvalidInputErrorKind::NoConfidence(encoding_name.clone()),
                format!(
                    "no confidence (predicted: {})",
                    &encoding_name,
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
        fallback_encoding: opt.fallback_encoding,
        json: opt.json,
        stat: opt.stat,
    };
    let mut paths = opt.paths;
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
