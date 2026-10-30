use anchor_surface::diff::{self, Severity};
use anchor_surface::idl;
use anchor_surface::report::{self, Side};
use std::process::ExitCode;

const USAGE: &str = "\
anchor-surface: diff two Anchor IDLs and flag changes to who can call what

USAGE:
    anchor-surface [OPTIONS] <OLD_IDL.json> <NEW_IDL.json>

OPTIONS:
    --format <text|json>                  Output format [default: text]
    --fail-on <danger|review|info|never>  Exit with code 1 if any finding is at
                                          or above this severity [default: danger]
    -h, --help                            Print this help
    -V, --version                         Print the version

EXIT CODES:
    0   no findings at or above --fail-on
    1   at least one finding at or above --fail-on
    2   usage error, or a file could not be read or is not an Anchor IDL
";

struct Options {
    old: String,
    new: String,
    json: bool,
    fail_on: Option<Severity>,
}

fn parse_args(args: &[String]) -> Result<Option<Options>, String> {
    let mut paths = Vec::new();
    let mut json = false;
    let mut fail_on = Some(Severity::Danger);
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match a {
            "-h" | "--help" => return Ok(None),
            "-V" | "--version" => {
                println!("anchor-surface {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--format" => match value("--format")?.as_str() {
                "text" => json = false,
                "json" => json = true,
                other => return Err(format!("unknown --format `{other}` (use text or json)")),
            },
            "--fail-on" => {
                let v = value("--fail-on")?;
                fail_on = if v == "never" {
                    None
                } else {
                    Some(Severity::parse(&v).ok_or_else(|| format!("unknown --fail-on `{v}`"))?)
                };
            }
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option `{s}`")),
            _ => paths.push(a.to_string()),
        }
        i += 1;
    }
    match <[String; 2]>::try_from(paths) {
        Ok([old, new]) => Ok(Some(Options {
            old,
            new,
            json,
            fail_on,
        })),
        Err(_) => Err("expected exactly two IDL files: <OLD_IDL.json> <NEW_IDL.json>".into()),
    }
}

fn load(path: &str) -> Result<idl::Program, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    idl::load(&text).map_err(|e| format!("{path}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(Some(o)) => o,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let (old, new) = match (load(&opts.old), load(&opts.new)) {
        (Ok(o), Ok(n)) => (o, n),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let report = diff::diff(&old, &new);
    let old_side = Side {
        path: &opts.old,
        program: &old,
    };
    let new_side = Side {
        path: &opts.new,
        program: &new,
    };
    if opts.json {
        print!("{}", report::json(&report, &old_side, &new_side));
    } else {
        print!("{}", report::text(&report, &old_side, &new_side));
    }

    let failed = match opts.fail_on {
        Some(level) => report.findings.iter().any(|f| f.severity <= level),
        None => false,
    };
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_options() {
        let o = parse_args(&args(&[
            "a.json",
            "--format",
            "json",
            "b.json",
            "--fail-on",
            "review",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            (o.old.as_str(), o.new.as_str(), o.json),
            ("a.json", "b.json", true)
        );
        assert_eq!(o.fail_on, Some(Severity::Review));
        let o = parse_args(&args(&["a", "b", "--fail-on", "never"]))
            .unwrap()
            .unwrap();
        assert_eq!(o.fail_on, None);
    }

    #[test]
    fn rejects_bad_usage() {
        assert!(parse_args(&args(&["only-one.json"])).is_err());
        assert!(parse_args(&args(&["a", "b", "c"])).is_err());
        assert!(parse_args(&args(&["a", "b", "--format", "xml"])).is_err());
        assert!(parse_args(&args(&["a", "b", "--fail-on"])).is_err());
        assert!(parse_args(&args(&["a", "b", "--nope"])).is_err());
        assert!(parse_args(&args(&["--help"])).unwrap().is_none());
    }
}
