//! quire: a Cargo-like build tool and package manager for LaTeX.

mod compile;
mod diagnostics;
mod doctor;
mod edit;
mod filelock;
mod git;
mod lock;
mod manifest;
mod pack;
mod preamble;
mod project;
mod resolve;
mod scaffold;
mod shell;
mod watch;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

use compile::BuildRequest;
use diagnostics::{MessageFormat, Reporter};
use project::Project;
use resolve::{Policy, Update};

#[derive(Parser)]
#[command(name = "quire", version, about = "A Cargo-like build tool and package manager for LaTeX")]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Fail instead of changing quire.lock
    #[arg(long, global = true)]
    locked: bool,
    /// Do not access the network; use only cached dependencies
    #[arg(long, global = true)]
    offline: bool,
    /// Same as --locked --offline
    #[arg(long, global = true)]
    frozen: bool,
}

impl Cli {
    fn policy(&self) -> Policy {
        Policy { locked: self.locked || self.frozen, offline: self.offline || self.frozen }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Create a new project in a new directory
    New {
        path: PathBuf,
        #[command(flatten)]
        args: scaffold::NewArgs,
    },
    /// Create a new project in the current directory (adopts an existing .tex file)
    Init {
        #[command(flatten)]
        args: scaffold::NewArgs,
    },
    /// Compile the document
    #[command(visible_alias = "b")]
    Build {
        #[command(flatten)]
        build: BuildArgs,
        /// Open the PDF after a successful build
        #[arg(long)]
        open: bool,
    },
    /// Rebuild whenever the project or a path dependency changes
    #[command(visible_alias = "w")]
    Watch {
        #[command(flatten)]
        build: BuildArgs,
    },
    /// Bundle the sources for submission, checked to compile on their own
    Pack {
        /// Where the bundle is going
        #[arg(long, default_value = "arxiv")]
        target: String,
        /// Build profile to pack (default: the target's `profile`, else release)
        #[arg(long)]
        profile: Option<String>,
        /// Skip compiling the bundle in isolation
        #[arg(long)]
        no_verify: bool,
        /// Show full latexmk output and every packed file
        #[arg(long, short = 'v')]
        verbose: bool,
    },
    /// Remove build output
    Clean {
        /// Remove only this profile's output
        #[arg(long)]
        profile: Option<String>,
    },
    /// Add a dependency to quire.toml
    Add(edit::AddArgs),
    /// Remove a dependency from quire.toml
    #[command(visible_alias = "rm")]
    Remove { name: String },
    /// Download git dependencies and write quire.lock, without building
    Fetch,
    /// Move git dependencies to the newest commit their branch/tag allows
    Update {
        /// Only update these dependencies
        names: Vec<String>,
    },
    /// Check that the tools quire relies on are installed
    Doctor,
}

#[derive(Args)]
struct BuildArgs {
    /// Build profile (default: dev)
    #[arg(long, conflicts_with = "release")]
    profile: Option<String>,
    /// Use the release profile
    #[arg(long, short = 'r')]
    release: bool,
    /// Show the full latexmk output
    #[arg(long, short = 'v')]
    verbose: bool,
    /// Output format for diagnostics: `json` prints one JSON object per line on stdout
    #[arg(long, value_enum, default_value_t = MessageFormat::Human)]
    message_format: MessageFormat,
}

impl BuildArgs {
    fn request(&self) -> BuildRequest {
        let profile =
            if self.release { "release".to_string() } else { self.profile.clone().unwrap_or_else(|| "dev".into()) };
        BuildRequest { profile, verbose: self.verbose, reporter: Reporter { format: self.message_format } }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            shell::error(&e);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<bool> {
    let policy = cli.policy();
    match cli.command {
        Command::New { path, args } => scaffold::new_project(&path, &args)?,
        Command::Init { args } => scaffold::init_project(&std::env::current_dir()?, &args)?,
        Command::Build { build, open } => {
            let req = build.request();
            let result = Project::discover().and_then(|project| {
                let resolution = resolve::resolve(&project, &Update::None, policy)?;
                compile::build(&project, &resolution, &req)
            });
            req.reporter.finished(result.is_ok());
            let output = result?;
            if open {
                open_file(&output.pdf)?;
            }
        }
        Command::Watch { build } => {
            let project = Project::discover()?;
            watch::watch(&project.root, &build.request(), policy)?;
        }
        Command::Pack { target, profile, no_verify, verbose } => {
            let project = Project::discover()?;
            let resolution = resolve::resolve(&project, &Update::None, policy)?;
            let req = pack::PackRequest {
                target,
                profile,
                verify: !no_verify,
                verbose,
                reporter: Reporter { format: MessageFormat::Human },
            };
            pack::pack(&project, &resolution, &req)?;
        }
        Command::Clean { profile } => {
            let project = Project::discover()?;
            let dir = match &profile {
                Some(p) => project.target_dir().join(p),
                None => project.target_dir(),
            };
            if dir.exists() {
                fs::remove_dir_all(&dir).with_context(|| format!("failed to remove `{}`", dir.display()))?;
                shell::status("Removed", project.display(&dir));
            }
        }
        Command::Add(args) => edit::add(&Project::discover()?, &args, policy)?,
        Command::Remove { name } => edit::remove(&Project::discover()?, &name, policy)?,
        Command::Fetch => {
            let project = Project::discover()?;
            let resolution = resolve::resolve(&project, &Update::None, policy)?;
            report_deps(&resolution);
        }
        Command::Update { names } => {
            let project = Project::discover()?;
            for name in &names {
                if !project.manifest.dependencies.contains_key(name) {
                    bail!("`{name}` is not a direct dependency of `{}`", project.manifest.name());
                }
            }
            let update = if names.is_empty() { Update::All } else { Update::Only(names) };
            let resolution = resolve::resolve(&project, &update, policy)?;
            report_deps(&resolution);
        }
        Command::Doctor => return Ok(doctor::doctor()),
    }
    Ok(true)
}

fn report_deps(resolution: &resolve::Resolution) {
    for dep in &resolution.deps {
        let source = match &dep.source {
            resolve::Source::TexLive { version } => format!("TeX distribution ({version})"),
            resolve::Source::Path => {
                format!("path {}", dep.root.as_ref().map(|p| project::tex_path(p)).unwrap_or_default())
            }
            resolve::Source::Git { url, rev } => format!("git {url} @ {}", &rev[..rev.len().min(12)]),
        };
        shell::status("Resolved", format!("{} — {source} (required by {})", dep.name, dep.required_by));
    }
    if resolution.deps.is_empty() {
        shell::status("Resolved", "no dependencies");
    }
}

fn open_file(path: &Path) -> Result<()> {
    let mut cmd = if cfg!(windows) {
        let mut c = Process::new("cmd");
        c.args(["/C", "start", ""]).arg(path);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Process::new("open");
        c.arg(path);
        c
    } else {
        let mut c = Process::new("xdg-open");
        c.arg(path);
        c
    };
    cmd.spawn().with_context(|| format!("failed to open `{}`", path.display()))?;
    Ok(())
}
