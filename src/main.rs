use std::env;
use std::fs;
use std::path::Path;
use std::process;

use cargo_lock::{Lockfile, Package};

use cargo2port::{
    format_cargo_crates, lockfile_from_path, lockfile_from_stdin, resolve_lockfile_packages,
    splice_cargo_crates, AlignmentMode, Result,
};

fn main() {
    let mut mode = AlignmentMode::Normal;
    let mut files: Vec<String> = vec![];
    let mut portfile_path: Option<String> = None;
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match &arg[..] {
            "" => continue,
            "--help" => print_usage(0),
            "-?" => print_usage(0),
            "-h" => print_usage(0),
            "--align=maxlen" => mode = AlignmentMode::Maxlen,
            "--align=multiline" => mode = AlignmentMode::Multiline,
            "--align=justify" => mode = AlignmentMode::Justify,
            "-P" | "--portfile" => {
                portfile_path = Some(args.next().unwrap_or_else(|| {
                    eprintln!("Error: -P requires a path to a Portfile");
                    process::exit(1);
                }));
            }
            _ => match check_path(&arg[..]) {
                Some(path) => files.push(path),
                None => process::exit(1),
            },
        }
    }

    if files.is_empty() {
        files.push("Cargo.lock".to_string())
    }

    match read_packages_from_lockfiles(&files) {
        Ok(packages) => {
            if packages.is_empty() {
                eprintln!("No packages with checksums found.");
                process::exit(0);
            }

            let block = format_cargo_crates(packages, mode);

            match portfile_path {
                Some(ref path) => update_portfile(path, &block),
                None => println!("{}", block),
            }
        }
        Err(error) => {
            eprintln!("{}", error);
            process::exit(1)
        }
    }
}

fn update_portfile(path: &str, cargo_crates_block: &str) {
    let portfile_path = Path::new(path);

    let contents = fs::read_to_string(portfile_path).unwrap_or_else(|e| {
        eprintln!("Error reading Portfile '{}': {}", path, e);
        process::exit(1);
    });

    let updated = splice_cargo_crates(&contents, cargo_crates_block).unwrap_or_else(|| {
        eprintln!("Error: no cargo.crates block found in '{}'", path);
        process::exit(1);
    });

    // Write to a temporary file in the same directory, then rename into place
    // so the Portfile is never left in a partially-written state.
    let dir = portfile_path.parent().unwrap_or(Path::new("."));
    let tmp_path = dir.join(".Portfile.cargo2port.tmp");

    fs::write(&tmp_path, &updated).unwrap_or_else(|e| {
        eprintln!("Error writing temporary file '{}': {}", tmp_path.display(), e);
        process::exit(1);
    });

    fs::rename(&tmp_path, portfile_path).unwrap_or_else(|e| {
        let _ = fs::remove_file(&tmp_path);
        eprintln!("Error replacing Portfile '{}': {}", path, e);
        process::exit(1);
    });

    eprintln!("Updated {}", path);
}

fn check_path(arg: &str) -> Option<String> {
    if arg == "-" {
        return Some(arg.to_string());
    }

    let path = Path::new(&arg);
    match path.try_exists() {
        Ok(true) => {
            if path.is_file() {
                match path.to_str() {
                    Some(path_str) => Some(path_str.to_string()),
                    None => process::exit(1),
                }
            } else {
                match path.join("Cargo.lock").to_str() {
                    Some(file_path) => check_path(file_path),
                    None => {
                        eprintln!("Error: failure appending Cargo.lock to {arg}");
                        process::exit(1);
                    }
                }
            }
        }
        Ok(false) => {
            eprintln!("Error: cannot find file {arg}");
            process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

fn read_packages_from_lockfiles(files: &Vec<String>) -> Result<Vec<Package>> {
    let mut lockfiles: Vec<Lockfile> = vec![];

    for name in files {
        let lockfile = if name == "-" {
            lockfile_from_stdin()?
        } else {
            lockfile_from_path(name)?
        };

        lockfiles.push(lockfile);
    }

    resolve_lockfile_packages(&lockfiles)
}

fn print_usage(code: i32) {
    let arg0 = env::args().next().unwrap_or("cargo2port".to_owned());
    eprintln!(
        "Usage: {} [options] <path/to/Cargo.lock>...

Generate a cargo.crates block for a MacPorts Portfile from one or more
Cargo.lock files. By default the block is printed to stdout.

Options:
  -h, --help                            Print this help message
  --align=maxlen|multiline|justify      Set alignment mode
  -P, --portfile <path>                 Update the cargo.crates block in <path> in place",
        arg0
    );
    process::exit(code);
}
