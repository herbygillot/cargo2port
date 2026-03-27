use std::env;
use std::fs;
use std::path::Path;
use std::process::{self, Command};

use cargo_lock::{Lockfile, Package};

use cargo2port::{
    find_cargo_lock, format_cargo_crates, lockfile_from_path, lockfile_from_stdin,
    resolve_lockfile_packages, splice_cargo_crates, AlignmentMode, Result,
};

/// Conditionally print a debug message to stderr.
macro_rules! debug {
    ($debug:expr, $($arg:tt)*) => {
        if $debug {
            eprintln!("[debug] {}", format!($($arg)*));
        }
    };
}

/// Parsed command-line options.
struct Options {
    mode: AlignmentMode,
    portfile_mode: bool,
    output_path: Option<String>,
    debug: bool,
    files: Vec<String>,
}

/// Parse an alignment mode string, exiting on invalid input.
fn parse_align_mode(value: &str) -> AlignmentMode {
    match value {
        "maxlen" => AlignmentMode::Maxlen,
        "multiline" => AlignmentMode::Multiline,
        "justify" => AlignmentMode::Justify,
        other => {
            eprintln!("Error: unknown alignment mode: {}", other);
            process::exit(1);
        }
    }
}

/// Consume the next argument (or the remainder of a combined flag) as a value
/// for a flag that requires one.
///
/// `chars` and `char_idx` are for combined short flags (e.g. `-Po-`): if there
/// are characters remaining after the flag letter, they are used as the value.
/// Otherwise the next top-level argument is consumed via `args`/`arg_idx`.
fn consume_flag_value(
    flag: &str,
    chars: &[char],
    char_idx: usize,
    args: &[String],
    arg_idx: &mut usize,
) -> String {
    let rest: String = chars[char_idx + 1..].iter().collect();
    if !rest.is_empty() {
        return rest;
    }

    *arg_idx += 1;
    if *arg_idx >= args.len() {
        eprintln!("Error: {} requires an argument", flag);
        process::exit(1);
    }
    args[*arg_idx].clone()
}

/// Parse command-line arguments into an [`Options`] struct.
fn parse_args() -> Options {
    let mut opts = Options {
        mode: AlignmentMode::Normal,
        portfile_mode: false,
        output_path: None,
        debug: false,
        files: vec![],
    };

    let args: Vec<String> = env::args().skip(1).collect();
    let mut i = 0;

    while i < args.len() {
        match &args[i][..] {
            "" => {}
            "--help" | "-?" | "-h" => print_usage(0),
            "--align=maxlen" => opts.mode = AlignmentMode::Maxlen,
            "--align=multiline" => opts.mode = AlignmentMode::Multiline,
            "--align=justify" => opts.mode = AlignmentMode::Justify,
            "--debug" => opts.debug = true,
            "--portfile" | "-P" => opts.portfile_mode = true,
            "-l" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: -l requires an argument (maxlen, multiline, justify)");
                    process::exit(1);
                }
                opts.mode = parse_align_mode(&args[i]);
            }
            "-o" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: -o requires an argument (path or -)");
                    process::exit(1);
                }
                opts.output_path = Some(args[i].clone());
            }
            arg => {
                // Handle combined short flags like -Po, -Pl, etc.
                if arg.starts_with('-') && !arg.starts_with("--") && arg.len() > 2 {
                    let chars: Vec<char> = arg[1..].chars().collect();
                    let mut j = 0;
                    while j < chars.len() {
                        match chars[j] {
                            'P' => opts.portfile_mode = true,
                            'h' | '?' => print_usage(0),
                            'o' => {
                                opts.output_path =
                                    Some(consume_flag_value("-o", &chars, j, &args, &mut i));
                                break;
                            }
                            'l' => {
                                let val = consume_flag_value("-l", &chars, j, &args, &mut i);
                                opts.mode = parse_align_mode(&val);
                                break;
                            }
                            _ => {
                                eprintln!("Error: unknown flag: -{}", chars[j]);
                                process::exit(1);
                            }
                        }
                        j += 1;
                    }
                } else {
                    opts.files.push(arg.to_string());
                }
            }
        }
        i += 1;
    }

    if !opts.portfile_mode && opts.output_path.is_some() {
        eprintln!("Error: -o requires --portfile (-P)");
        process::exit(1);
    }

    opts
}

fn main() {
    let opts = parse_args();

    debug!(opts.debug, "args: {:?}", env::args().collect::<Vec<_>>());
    debug!(
        opts.debug,
        "portfile_mode={}, files={:?}, output_path={:?}",
        opts.portfile_mode,
        opts.files,
        opts.output_path
    );

    if opts.portfile_mode {
        run_portfile_mode(opts.files, opts.mode, opts.output_path, opts.debug);
    } else {
        run_lockfile_mode(opts.files, opts.mode, opts.debug);
    }
}

/// Read one or more Cargo.lock files and print the generated `cargo.crates` block to stdout.
fn run_lockfile_mode(files: Vec<String>, mode: AlignmentMode, debug: bool) {
    let mut validated: Vec<String> = vec![];

    for file in &files {
        debug!(debug, "validating lockfile path: {}", file);
        match check_path(file) {
            Some(path) => validated.push(path),
            None => process::exit(1),
        }
    }

    if validated.is_empty() {
        validated.push("Cargo.lock".to_string());
    }

    debug!(debug, "lockfile paths: {:?}", validated);

    let files = validated;

    match read_packages_from_lockfiles(&files) {
        Ok(packages) => {
            if packages.is_empty() {
                eprintln!("No packages with checksums found.");
                process::exit(0);
            }

            debug!(debug, "found {} packages", packages.len());
            println!("{}", format_cargo_crates(packages, mode));
        }
        Err(error) => {
            eprintln!("{}", error);
            process::exit(1)
        }
    }
}

/// Portfile mode: extract the port if needed, locate the Cargo.lock in the work directory,
/// generate a `cargo.crates` block, and splice it into the Portfile.
///
/// By default the Portfile is edited in place. If `-o` was provided, the updated contents
/// are written to the given path (or stdout for `-o -`).
fn run_portfile_mode(
    mut files: Vec<String>,
    mode: AlignmentMode,
    output_path: Option<String>,
    debug: bool,
) {
    // Verify `port` is available before doing anything else
    if let Err(e) = Command::new("port").arg("version").output() {
        eprintln!("Error: 'port' command not found. Is MacPorts installed and in your PATH?");
        debug!(debug, "port version failed: {}", e);
        process::exit(1);
    }

    if files.is_empty() {
        files.push("Portfile".to_string());
    }

    if files.len() > 1 {
        eprintln!("Error: --portfile mode expects a single Portfile path");
        process::exit(1);
    }

    debug!(debug, "raw file argument: {}", files[0]);

    // If the argument is a directory, look for Portfile inside it
    let raw_path = Path::new(&files[0]);
    let resolved = if raw_path.is_dir() {
        debug!(debug, "'{}' is a directory, appending Portfile", files[0]);
        raw_path.join("Portfile")
    } else {
        debug!(debug, "'{}' is not a directory, using as-is", files[0]);
        raw_path.to_path_buf()
    };

    debug!(debug, "resolved path: {}", resolved.display());

    let portfile_path = fs::canonicalize(&resolved).unwrap_or_else(|e| {
        eprintln!(
            "Error: cannot resolve Portfile path '{}': {}",
            resolved.display(),
            e
        );
        process::exit(1);
    });

    debug!(debug, "canonical portfile path: {}", portfile_path.display());

    if !portfile_path.is_file() {
        eprintln!("Error: not a file: {}", portfile_path.display());
        process::exit(1);
    }

    let portfile_dir = portfile_path.parent().unwrap_or_else(|| {
        eprintln!("Error: cannot determine directory of Portfile");
        process::exit(1);
    });

    debug!(debug, "portfile directory: {}", portfile_dir.display());

    // Get the work directory from `port work`, run in the Portfile's directory
    let mut workpath = get_port_workpath(portfile_dir, debug);

    if workpath.is_empty() {
        eprintln!("Work directory does not exist. Running port extract...");
        run_port_extract(portfile_dir, debug);
        workpath = get_port_workpath(portfile_dir, debug);
        if workpath.is_empty() {
            eprintln!("Error: work directory still does not exist after port extract");
            process::exit(1);
        }
    }

    debug!(debug, "workpath: {}", workpath);

    let workpath = Path::new(workpath.trim());

    if !workpath.is_dir() {
        eprintln!(
            "Error: work directory does not exist on disk: {}",
            workpath.display()
        );
        process::exit(1);
    }

    debug!(debug, "searching for Cargo.lock under {}", workpath.display());

    // Find Cargo.lock in the work directory
    let cargo_lock_path = find_cargo_lock(workpath).unwrap_or_else(|| {
        eprintln!(
            "Error: no Cargo.lock found alongside Cargo.toml under {}",
            workpath.display()
        );
        process::exit(1);
    });

    eprintln!("Using {}", cargo_lock_path.display());

    // Parse the Cargo.lock and generate the cargo.crates block
    let lockfile_path_str = cargo_lock_path.to_string_lossy();
    let lockfile_files = vec![lockfile_path_str.to_string()];

    debug!(debug, "reading lockfile: {}", lockfile_path_str);

    let packages = match read_packages_from_lockfiles(&lockfile_files) {
        Ok(packages) => packages,
        Err(error) => {
            eprintln!("Error reading Cargo.lock: {}", error);
            process::exit(1);
        }
    };

    debug!(debug, "found {} packages with checksums", packages.len());

    if packages.is_empty() {
        eprintln!("No packages with checksums found.");
        process::exit(0);
    }

    let cargo_crates_block = format_cargo_crates(packages, mode);

    debug!(
        debug,
        "generated cargo.crates block ({} bytes)",
        cargo_crates_block.len()
    );

    // Read the Portfile and splice in the new block
    debug!(debug, "reading portfile: {}", portfile_path.display());

    let portfile_contents = fs::read_to_string(&portfile_path).unwrap_or_else(|e| {
        eprintln!("Error reading Portfile: {}", e);
        process::exit(1);
    });

    debug!(
        debug,
        "portfile contents: {} bytes, {} lines",
        portfile_contents.len(),
        portfile_contents.lines().count()
    );

    let (updated, appended) = splice_cargo_crates(&portfile_contents, &cargo_crates_block);

    if appended {
        eprintln!("Portfile does not contain cargo.crates - appending.");
    }

    debug!(
        debug,
        "spliced portfile: {} bytes (appended={})",
        updated.len(),
        appended
    );

    // Determine where to write the output
    match output_path {
        Some(ref path) if path == "-" => {
            debug!(debug, "writing to stdout");
            print!("{}", updated);
        }
        Some(ref path) => {
            debug!(debug, "writing to {}", path);
            fs::write(path, &updated).unwrap_or_else(|e| {
                eprintln!("Error writing to '{}': {}", path, e);
                process::exit(1);
            });
            eprintln!("Wrote updated Portfile to {}", path);
        }
        None => {
            debug!(debug, "writing in place to {}", portfile_path.display());
            fs::write(&portfile_path, &updated).unwrap_or_else(|e| {
                eprintln!("Error writing Portfile: {}", e);
                process::exit(1);
            });
            eprintln!("Updated {}", portfile_path.display());
        }
    }
}

/// Run `port work` in the given directory and return the output (trimmed).
///
/// Returns an empty string if the work directory does not exist (port work exits
/// successfully but prints nothing). Exits on command failure.
fn get_port_workpath(portfile_dir: &Path, debug: bool) -> String {
    debug!(debug, "running: port work (in {})", portfile_dir.display());

    let output = Command::new("port")
        .arg("work")
        .current_dir(portfile_dir)
        .output()
        .unwrap_or_else(|e| {
            eprintln!("Error running 'port work': {}", e);
            process::exit(1);
        });

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    debug!(debug, "port work stdout: {:?}", stdout);
    if !stderr.is_empty() {
        debug!(debug, "port work stderr: {:?}", stderr);
    }
    debug!(debug, "port work exit status: {}", output.status);

    if !output.status.success() {
        eprintln!("Error: 'port work' failed (exit status: {})", output.status);
        if !stderr.is_empty() {
            eprintln!("{}", stderr);
        }
        process::exit(1);
    }

    stdout
}

/// Check if the current process is running as root by invoking `id -u`.
fn is_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
        .unwrap_or(false)
}

/// Run `port extract` in the given directory. Uses `sudo` unless already elevated.
fn run_port_extract(portfile_dir: &Path, debug: bool) {
    let elevated = is_root();

    debug!(debug, "elevated (root): {}", elevated);

    let (cmd_name, args) = if elevated {
        ("port", vec!["extract"])
    } else {
        ("sudo", vec!["port", "extract"])
    };

    debug!(
        debug,
        "running: {} {} (in {})",
        cmd_name,
        args.join(" "),
        portfile_dir.display()
    );

    let status = Command::new(cmd_name)
        .args(&args)
        .current_dir(portfile_dir)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    match status {
        Ok(s) if s.success() => {
            debug!(debug, "port extract succeeded");
        }
        Ok(s) => {
            eprintln!("port extract exited with status: {}", s);
            process::exit(1);
        }
        Err(e) => {
            eprintln!("Error running port extract: {}", e);
            process::exit(1);
        }
    }
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
        "Usage: {} [options] [file ...]

Options:
  -h, --help                              Print this help message
  -l, --align=maxlen|multiline|justify
                                          Set alignment mode
  -P, --portfile                          Portfile mode: extract, find Cargo.lock,
                                          update cargo.crates block in place
  -o <path>                               Output to <path> instead of editing in place
                                          (use '-' for stdout; requires -P)
  --debug                                  Print debug information to stderr",
        arg0
    );
    process::exit(code);
}
