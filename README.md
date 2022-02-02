# detat

cat(1) with chardet.

![detat](images/example.gif)

## Usage

```
USAGE:
    detat [FLAGS] [OPTIONS] [PATH]...

FLAGS:
    -b, --allow-binary    Print a binary input as it is
    -h, --help            Prints help information
    -j, --json            Show results in a JSON Lines format
    -s, --stat            Show statistics
    -V, --version         Prints version information

OPTIONS:
    -c, --confidence-min <CONFIDENCE_MIN>    Fail if detected confidence is less than this [default: 0]
    -f, --fallback <ENCODING>                Use this encoding if detected confidence is less than <CONFIDENCE_MIN>

ARGS:
    <PATH>...    An input file
```

## Installation

```sh
git clone https://github.com/taskie/detat.git
cd detat
cargo install --path .
```

## License

LGPL-3.0
