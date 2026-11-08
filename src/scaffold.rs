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
    /// TeX engine [default: pdflatex; `init` detects it from the packages loaded]
    #[arg(long, value_enum)]
    pub engine: Option<Engine>,
    /// Project name (defaults to the directory name)
    #[arg(long)]
    pub name: Option<String>,
    /// Initialize a version control repository
    #[arg(long, value_enum, default_value_t = Vcs::Git)]
    pub vcs: Vcs,
}

pub fn new_project(path: &Path, args: &NewArgs) -> Result<()> {
    if path.exists() && fs::read_dir(path)?.next().is_some() {
        bail!(
            "destination `{}` already exists and is not empty\nhelp: use `quire init` inside it instead",
            path.display()
        );
    }
    let name = project_name(path, args)?;
    fs::create_dir_all(path).with_context(|| format!("failed to create `{}`", path.display()))?;
    let files = template_files(&name, args.template, args.engine.unwrap_or(Engine::Pdflatex), None);
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
    let mut engine = args.engine.unwrap_or(Engine::Pdflatex);
    if let Some(found) = &existing {
        shell::note(format!("using existing `{}` as the main file", found.main.display()));
        if !found.others.is_empty() {
            let others: Vec<String> = found.others.iter().map(|p| format!("`{}`", p.display())).collect();
            shell::note(format!("also found {}; change `main` in {MANIFEST_NAME} to build another", others.join(", ")));
        }
        if args.engine.is_none()
            && let Some(detected) = found.engine
        {
            engine = detected;
            shell::note(format!("using {} for the packages the document loads", detected.as_str()));
        }
    }
    let existing = existing.map(|found| found.main);
    let files = template_files(&name, args.template, engine, existing.as_deref());
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

/// A document `quire init` found in an existing directory.
#[derive(Debug, PartialEq)]
struct Existing {
    /// Relative to the project directory.
    main: PathBuf,
    /// The engine the document needs, when it says so.
    engine: Option<Engine>,
    /// Other complete documents that could have been the main file.
    others: Vec<PathBuf>,
}

/// The document's main file, in the directory or its `src/`: the one arXiv's
/// `00README` names, else the best `.tex` file with an uncommented
/// `\documentclass` and `\begin{document}`. Standalone figures and files that
/// another candidate `\input`s rank last, conventional names (`main.tex`,
/// `paper.tex`, ...) first, then the largest file wins.
fn find_existing_main(dir: &Path) -> Option<Existing> {
    let (readme_main, readme_engine) = arxiv_readme(dir);
    if let Some(main) = readme_main {
        let engine = readme_engine.or_else(|| read_tex(&dir.join(&main)).as_deref().and_then(engine_for));
        return Some(Existing { main, engine, others: vec![] });
    }
    for sub in [Path::new("."), Path::new("src")] {
        let Ok(entries) = fs::read_dir(dir.join(sub)) else { continue };
        let mut docs: Vec<(PathBuf, String)> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tex")))
            .filter_map(|p| read_tex(&p).map(|text| (p, text)))
            .filter(|(_, text)| text.contains("\\documentclass") && text.contains("\\begin{document}"))
            .collect();
        if docs.is_empty() {
            continue;
        }
        let referenced: Vec<String> = docs.iter().flat_map(|(_, text)| inputs(text)).collect();
        docs.sort_by_cached_key(|(path, text)| {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
            let secondary =
                crate::compile::document_class(text).as_deref() == Some("standalone") || referenced.contains(&stem);
            let conventional = ["main", "paper", "ms", "manuscript", "article", "thesis"].contains(&stem.as_str());
            (secondary, !conventional, std::cmp::Reverse(text.len()), path.clone())
        });
        let relative = |path: &Path| {
            let file = path.file_name().unwrap();
            if sub == Path::new(".") { PathBuf::from(file) } else { sub.join(file) }
        };
        let engine = engine_for(&docs[0].1);
        return Some(Existing {
            main: relative(&docs[0].0),
            engine,
            others: docs[1..].iter().map(|(p, _)| relative(p)).collect(),
        });
    }
    None
}

/// A `.tex` file without its comments. Not every paper is UTF-8 (Latin-1 is
/// common on arXiv), so invalid bytes are replaced rather than rejected.
fn read_tex(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(crate::pack::strip_tex_comments(&String::from_utf8_lossy(&bytes)))
}

/// Names of the files `\input`/`\include`d, lowercased and without `.tex`.
fn inputs(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for command in ["\\input{", "\\include{", "\\includestandalone{", "\\subfile{"] {
        for (at, _) in text.match_indices(command) {
            let rest = &text[at + command.len()..];
            if let Some(end) = rest.find('}') {
                let name = rest[..end].trim().to_lowercase();
                let name = name.strip_suffix(".tex").unwrap_or(&name);
                names.push(name.rsplit('/').next().unwrap_or(name).to_string());
            }
        }
    }
    names
}

/// The engine a document needs, judged from the packages it loads.
fn engine_for(text: &str) -> Option<Engine> {
    let loads = |package: &str| {
        text.match_indices("\\usepackage").any(|(at, _)| {
            let rest = &text[at..];
            let end = rest.find('}').unwrap_or(rest.len());
            let names = rest[..end].rsplit('{').next().unwrap_or("");
            names.split(',').any(|n| n.trim() == package)
        })
    };
    if text.contains("\\directlua") || loads("luacode") || loads("luatexbase") {
        Some(Engine::Lualatex)
    } else if loads("fontspec") || loads("unicode-math") || loads("polyglossia") {
        Some(Engine::Xelatex)
    } else {
        None
    }
}

/// The top-level file and compiler from arXiv's `00README.json` or the older
/// `00README.XXX`, when they name a file that exists.
fn arxiv_readme(dir: &Path) -> (Option<PathBuf>, Option<Engine>) {
    let mut main = None;
    let mut engine = None;
    if let Ok(text) = fs::read_to_string(dir.join("00README.json"))
        && let Ok(spec) = serde_json::from_str::<serde_json::Value>(&text)
    {
        main = spec["sources"].as_array().into_iter().flatten().find_map(|source| {
            (source["usage"] == "toplevel").then(|| source["filename"].as_str()).flatten().map(PathBuf::from)
        });
        engine = match spec["process"]["compiler"].as_str() {
            Some("xelatex") => Some(Engine::Xelatex),
            Some("lualatex") => Some(Engine::Lualatex),
            _ => None,
        };
    }
    if main.is_none()
        && let Ok(text) = fs::read_to_string(dir.join("00README.XXX"))
    {
        main = text.lines().find_map(|line| match line.split_whitespace().collect::<Vec<_>>()[..] {
            [file, "toplevelfile"] => Some(PathBuf::from(file)),
            _ => None,
        });
    }
    (main.filter(|m| dir.join(m).is_file()), engine)
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

const PROFILE_LINE: &str =
    "% Defined by quire; the fallback keeps the file compilable without it.\n\\providecommand\\QuireProfile{none}\n";

fn template_files(
    name: &str,
    template: Template,
    engine: Engine,
    existing_main: Option<&Path>,
) -> Vec<(PathBuf, String)> {
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

    fn write(dir: &Path, files: &[(&str, &[u8])]) {
        for (name, text) in files {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    const PAPER: &[u8] =
        b"\\documentclass{article}\n\\begin{document}\nA long paper body. \\input{fig}\n\\end{document}\n";

    #[test]
    fn init_skips_standalone_figures_and_inputs() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            &[
                ("zz-paper.tex", PAPER),
                ("afig.tex", b"\\documentclass{standalone}\n\\begin{document}x\\end{document}\n"),
                ("fig.tex", b"\\documentclass{article}\n\\begin{document}x\\end{document}\n"),
                ("macros.tex", b"% \\documentclass{article}\n\\newcommand\\x{}\n"),
            ],
        );
        let found = find_existing_main(dir.path()).unwrap();
        assert_eq!(found.main, Path::new("zz-paper.tex"));
        assert_eq!(found.others.len(), 2);
        assert_eq!(found.engine, None);
    }

    #[test]
    fn init_prefers_conventional_names_then_size() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            &[("response.tex", PAPER), ("ms.tex", b"\\documentclass{x}\\begin{document}\\end{document}")],
        );
        assert_eq!(find_existing_main(dir.path()).unwrap().main, Path::new("ms.tex"));

        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &[("a.tex", b"\\documentclass{x}\\begin{document}\\end{document}"), ("b.tex", PAPER)]);
        assert_eq!(find_existing_main(dir.path()).unwrap().main, Path::new("b.tex"));
    }

    #[test]
    fn init_follows_the_arxiv_readme() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            &[
                ("main.tex", PAPER),
                ("other.tex", b"\\documentclass{article}\\usepackage{fontspec}\\begin{document}\\end{document}"),
                (
                    "00README.json",
                    br#"{"sources":[{"filename":"other.tex","usage":"toplevel"}],"process":{"compiler":"pdflatex"}}"#,
                ),
            ],
        );
        let found = find_existing_main(dir.path()).unwrap();
        assert_eq!(found.main, Path::new("other.tex"));
        assert_eq!(found.engine, Some(Engine::Xelatex));

        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &[("main.tex", PAPER), ("sub/p.tex", PAPER), ("00README.XXX", b"sub/p.tex toplevelfile\n")]);
        assert_eq!(find_existing_main(dir.path()).unwrap().main, Path::new("sub/p.tex"));
    }

    #[test]
    fn init_reads_latin1_and_detects_engines() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            &[("src/main.tex", b"\\documentclass{article}\n% Caf\xe9\n\\begin{document}\\end{document}\n")],
        );
        assert_eq!(find_existing_main(dir.path()).unwrap().main, Path::new("src/main.tex"));

        assert_eq!(engine_for("\\usepackage[T1]{fontenc}"), None);
        assert_eq!(engine_for("\\usepackage{amsmath,unicode-math}"), Some(Engine::Xelatex));
        assert_eq!(engine_for("\\usepackage{luacode}"), Some(Engine::Lualatex));
        assert_eq!(engine_for(&crate::pack::strip_tex_comments("%\\usepackage{fontspec}\n")), None);
        assert_eq!(inputs("\\input{figs/Plot.tex} \\include{ch1}"), ["plot", "ch1"]);
    }

    #[test]
    fn date_is_well_formed() {
        let d = today();
        assert_eq!(d.len(), 10);
        assert!(d.starts_with("20"));
    }
}
