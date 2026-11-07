//! `quire new` and `quire init`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::manifest::{Engine, MANIFEST_NAME, validate_name};
use crate::shell;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Template {
    Article,
    Report,
    Beamer,
    /// A reusable package of .sty/.cls files, with a documentation document
    Package,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Vcs {
    Git,
    None,
}

#[derive(Debug, clap::Args)]
pub struct NewArgs {
    /// Project template
    #[arg(long, value_enum, default_value_t = Template::Article)]
    pub template: Template,
    /// TeX engine
    #[arg(long, value_enum, default_value_t = Engine::Pdflatex)]
    pub engine: Engine,
    /// Project name (defaults to the directory name)
    #[arg(long)]
    pub name: Option<String>,
    /// Initialize a version control repository
    #[arg(long, value_enum, default_value_t = Vcs::Git)]
    pub vcs: Vcs,
}

pub fn new_project(path: &Path, args: &NewArgs) -> Result<()> {
    if path.exists() && fs::read_dir(path)?.next().is_some() {
        bail!("destination `{}` already exists and is not empty\nhelp: use `quire init` inside it instead", path.display());
    }
    let name = project_name(path, args)?;
    fs::create_dir_all(path).with_context(|| format!("failed to create `{}`", path.display()))?;
    let files = template_files(&name, args.template, args.engine, None);
    write_files(path, &files)?;
    init_vcs(path, args.vcs);
    shell::status("Created", format!("{} `{name}` in `{}`", describe(args.template), path.display()));
    Ok(())
}

pub fn init_project(dir: &Path, args: &NewArgs) -> Result<()> {
    if dir.join(MANIFEST_NAME).exists() {
        bail!("`{MANIFEST_NAME}` already exists in `{}`", dir.display());
    }
    let name = project_name(dir, args)?;
    // Adopt an existing document instead of adding a template next to it.
    let existing = if args.template == Template::Package { None } else { find_existing_main(dir) };
    if let Some(main) = &existing {
        shell::note(format!("using existing `{}` as the main file", main.display()));
    }
    let files = template_files(&name, args.template, args.engine, existing.as_deref());
    let files: Vec<(PathBuf, String)> = files.into_iter().filter(|(p, _)| !dir.join(p).exists()).collect();
    write_files(dir, &files)?;
    init_vcs(dir, args.vcs);
    shell::status("Created", format!("{} `{name}`", describe(args.template)));
    Ok(())
}

fn describe(template: Template) -> &'static str {
    match template {
        Template::Package => "package",
        _ => "document",
    }
}

fn project_name(path: &Path, args: &NewArgs) -> Result<String> {
    let name = match &args.name {
        Some(name) => name.clone(),
        None => {
            let absolute = std::path::absolute(path)?;
            absolute
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .context("cannot infer a project name; pass --name")?
        }
    };
    validate_name(&name).context("invalid project name\nhelp: pass a different one with --name")?;
    Ok(name)
}

/// A `.tex` file containing `\documentclass` in the directory or its `src/`.
fn find_existing_main(dir: &Path) -> Option<PathBuf> {
    for sub in [Path::new("."), Path::new("src")] {
        let Ok(entries) = fs::read_dir(dir.join(sub)) else { continue };
        let mut candidates: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "tex"))
            .filter(|p| fs::read_to_string(p).is_ok_and(|s| s.contains("\\documentclass")))
            .collect();
        candidates.sort();
        if let Some(found) = candidates.into_iter().next() {
            let file = found.file_name().unwrap();
            return Some(if sub == Path::new(".") { PathBuf::from(file) } else { sub.join(file) });
        }
    }
    None
}

fn write_files(root: &Path, files: &[(PathBuf, String)]) -> Result<()> {
    for (rel, contents) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, contents).with_context(|| format!("failed to write `{}`", path.display()))?;
    }
    Ok(())
}

fn init_vcs(dir: &Path, vcs: Vcs) {
    if vcs == Vcs::None {
        return;
    }
    let inside_repo = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|o| o.status.success());
    if inside_repo {
        return;
    }
    match Command::new("git").arg("-C").arg(dir).args(["init", "--quiet"]).status() {
        Ok(s) if s.success() => {}
        _ => shell::warn("could not run `git init`; continuing without a repository"),
    }
}

fn tex_escape(text: &str) -> String {
    text.replace('_', "\\_")
}

fn preamble(engine: Engine) -> &'static str {
    match engine {
        Engine::Pdflatex => "\\usepackage[T1]{fontenc}\n",
        Engine::Xelatex | Engine::Lualatex => "\\usepackage{fontspec}\n",
    }
}

const PROFILE_LINE: &str = "% Defined by quire; the fallback keeps the file compilable without it.\n\\providecommand\\QuireProfile{none}\n";

fn template_files(name: &str, template: Template, engine: Engine, existing_main: Option<&Path>) -> Vec<(PathBuf, String)> {
    let mut files = vec![(PathBuf::from(".gitignore"), "/target/\n".to_string())];
    let title = tex_escape(name);
    let pre = preamble(engine);

    if template == Template::Package {
        let macro_name: String = name.chars().filter(|c| c.is_ascii_alphabetic()).collect();
        let macro_name = if macro_name.is_empty() { "pkg".to_string() } else { macro_name };
        files.push((
            PathBuf::from(MANIFEST_NAME),
            format!(
                r#"[package]
name = "{name}"
version = "0.1.0"
description = ""
# Directory exposed to TeX for projects that depend on this package.
tex-dir = "tex"

# The package documentation, built with `quire build`.
[document]
name = "{name}-doc"
main = "doc/{name}-doc.tex"
engine = "{engine}"

[dependencies]
"#
            ),
        ));
        files.push((
            PathBuf::from(format!("tex/{name}.sty")),
            format!(
                "\\NeedsTeXFormat{{LaTeX2e}}\n\\ProvidesPackage{{{name}}}[{} v0.1.0 {title}]\n\n\\newcommand\\{macro_name}hello{{Hello from \\texttt{{{title}}}!}}\n\n\\endinput\n",
                today()
            ),
        ));
        files.push((
            PathBuf::from(format!("doc/{name}-doc.tex")),
            format!(
                "\\documentclass{{article}}\n{pre}\\usepackage{{{name}}}\n\n\\title{{The \\texttt{{{title}}} package}}\n\\date{{\\today}}\n\n\\begin{{document}}\n\\maketitle\n\n\\section{{Usage}}\n\\verb|\\usepackage{{{name}}}| provides \\verb|\\{macro_name}hello|: \\{macro_name}hello\n\\end{{document}}\n"
            ),
        ));
        return files;
    }

    let main = existing_main.map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|| "src/main.tex".into());
    files.push((
        PathBuf::from(MANIFEST_NAME),
        format!(
            r#"[document]
name = "{name}"
main = "{main}"
engine = "{engine}"

[dependencies]
# Packages from your TeX distribution:
# siunitx = "*"
# Local or git packages (anything with .sty/.cls files, or a quire [package]):
# lab-macros = {{ path = "../lab-macros" }}
# uni-thesis = {{ git = "https://github.com/org/uni-thesis", tag = "v1.0" }}

# `quire build --profile draft`
[profile.draft]
class-options = ["draft"]
"#
        ),
    ));
    if existing_main.is_some() {
        return files;
    }
    let body = match template {
        Template::Article => format!(
            "\\documentclass[11pt]{{article}}\n{pre}\n{PROFILE_LINE}\n\\title{{{title}}}\n\\author{{}}\n\\date{{\\today}}\n\n\\begin{{document}}\n\\maketitle\n\n\\section{{Introduction}}\nBuilt with quire (profile: \\QuireProfile).\n\n\\end{{document}}\n"
        ),
        Template::Report => format!(
            "\\documentclass[11pt]{{report}}\n{pre}\n{PROFILE_LINE}\n\\title{{{title}}}\n\\author{{}}\n\\date{{\\today}}\n\n\\begin{{document}}\n\\maketitle\n\\tableofcontents\n\n\\chapter{{Introduction}}\nBuilt with quire (profile: \\QuireProfile).\n\n\\end{{document}}\n"
        ),
        Template::Beamer => format!(
            "\\documentclass{{beamer}}\n{pre}\n{PROFILE_LINE}\n\\title{{{title}}}\n\\author{{}}\n\\date{{\\today}}\n\n\\begin{{document}}\n\n\\begin{{frame}}\n  \\titlepage\n\\end{{frame}}\n\n\\begin{{frame}}{{Introduction}}\n  Built with quire (profile: \\QuireProfile).\n\\end{{frame}}\n\n\\end{{document}}\n"
        ),
        Template::Package => unreachable!(),
    };
    files.push((PathBuf::from("src/main.tex"), body));
    files
}

/// Today's date as `YYYY/MM/DD`, the format `\ProvidesPackage` expects.
fn today() -> String {
    let days = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0) as i64;
    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}/{month:02}/{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;

    #[test]
    fn templates_produce_valid_manifests() {
        for template in [Template::Article, Template::Report, Template::Beamer, Template::Package] {
            let files = template_files("my_doc", template, Engine::Lualatex, None);
            let (_, manifest) = files.iter().find(|(p, _)| p == Path::new(MANIFEST_NAME)).unwrap();
            let m = Manifest::parse(manifest).unwrap();
            assert_eq!(m.name(), "my_doc");
            assert!(m.profile("dev").is_ok());
        }
    }

    #[test]
    fn date_is_well_formed() {
        let d = today();
        assert_eq!(d.len(), 10);
        assert!(d.starts_with("20"));
    }
}
