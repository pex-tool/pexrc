// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::anyhow;
use clap::Args;
use fs_err as fs;
use interpreter::Interpreter;
use pex::{Pex, PexPath};
use scripts::IdentifyInterpreter;
use zip::CompressionMethod;

#[derive(Args)]
pub(crate) struct ExtractArgs {
    /// The path to extract distribution as wheels to.
    #[arg(short = 'f', long, visible_aliases = ["find-links", "repo"])]
    dest_dir: PathBuf,

    /// Also extract a wheel for the PEX file sources.
    #[arg(short = 'D', long, default_value_t = false)]
    sources: bool,

    /// Use the current system time to generate timestamps for the extracted distributions.
    #[arg(
        long,
        default_value_t = false,
        long_help = "\
Use the current system time to generate timestamps for the extracted distributions. Otherwise, Pex
will use midnight on January 1, 1980. By using system time, the extracted distributions will not be
reproducible, meaning that if you were to re-run extraction against the same PEX file then the
newly extracted distributions would not be byte-for-byte identical distributions extracted in prior
runs."
    )]
    use_system_time: bool,

    /// Serve the `--find-links` repo.
    #[arg(long, default_value_t = false)]
    serve: bool,

    /// The port to serve the --find-links repo on.
    #[arg(long)]
    port: Option<u16>,

    /// The path of a file to write the `<pid>:<port>` of the find links server to.
    #[arg(long)]
    pid_file: Option<PathBuf>,
}

pub(crate) fn extract(python: &Path, pex: Pex, args: ExtractArgs) -> anyhow::Result<()> {
    // TODO: XXX: Plumb this or warn and ignore?
    // args.use_system_time

    pex::repackage_wheels(&pex, CompressionMethod::Deflated, None, &args.dest_dir)?;
    let pex_path = PexPath::from_pex_info(&pex.info, true);
    for additional_pex in pex_path.load_pexes()? {
        pex::repackage_wheels(
            &additional_pex,
            CompressionMethod::Deflated,
            None,
            &args.dest_dir,
        )?;
    }

    if args.sources {
        todo!("Creation of an sdist for PEX sources is not implemented yet.")
    }
    if args.serve {
        let mut scripts = pex.scripts()?;
        let identify_interpreter = IdentifyInterpreter::read(&mut scripts)?;
        let interpreter = Interpreter::load(python, &identify_interpreter)?;
        serve(
            &pex,
            &interpreter,
            &args.dest_dir,
            args.port,
            args.pid_file.as_deref(),
        )?;
    }
    Ok(())
}

fn serve(
    pex: &Pex,
    interpreter: &Interpreter,
    root_dir: &Path,
    port: Option<u16>,
    pid_file: Option<&Path>,
) -> anyhow::Result<()> {
    let module = if interpreter.version.major == 3 {
        "http.server"
    } else {
        "SimpleHTTPServer"
    };
    let mut child = Command::new(&interpreter.path)
        // N.B.: Running Python in unbuffered mode here is critical to being able to read stdout.
        .arg("-u")
        .args(["-m", module])
        .arg(port.unwrap_or_default().to_string())
        .current_dir(root_dir)
        .stdout(Stdio::piped())
        .spawn()?;
    let pid = child.id();
    let stdout = child.stdout.take().expect("We requested a stdout pipe.");
    let port_matcher = regex::Regex::new(r"^Serving HTTP on \S+ port (?P<port>\d+)\D")?;
    let (send, recv) = oneshot::channel::<anyhow::Result<u16>>();
    thread::spawn(move || {
        let mut stdout_lines = BufReader::new(stdout).lines();
        if let Some(line) = stdout_lines.next() {
            match line {
                Ok(line) => {
                    if let Some(captures) = port_matcher.captures(&line)
                        && let Some(needle) = captures.name("port")
                    {
                        send.send(needle.as_str().parse::<u16>().map_err(|err| {
                            anyhow!("Failed to parse HTTP server startup port: {err}")
                        }))?;
                    } else {
                        send.send(Err(anyhow!("Failed to start find-links HTTP server.")))?;
                    }
                }
                Err(err) => send.send(Err(anyhow!(
                    "Failed to read 1 line of output from {module} startup: {err}"
                )))?,
            }
        } else {
            send.send(Err(anyhow!(
                "Expected to read a least 1 line of output from {module} startup."
            )))?;
        }

        while let Some(Ok(line)) = stdout_lines.next() {
            eprintln!("{line}");
        }
        Ok::<_, anyhow::Error>(())
    });
    let port = recv.recv_timeout(Duration::from_secs(5))??;
    eprintln!(
        "Serving find-links repo of {pex} via {find_links} at http://localhost:{port}",
        pex = pex.path.display(),
        find_links = root_dir.display(),
    );

    if let Some(pid_file) = pid_file {
        fs::write(pid_file, format!("{pid}:{port}"))?;
    }
    if child.wait().is_err() {
        child.kill()?
    }
    Ok(())
}
