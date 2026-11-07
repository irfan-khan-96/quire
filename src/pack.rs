//! `quire pack --target arxiv`: turn a project into a self-contained source
//! bundle that compiles on arXiv.
//!
//! The document is built first with TeX's recorder on, so the `.fls` file lists
//! every file TeX actually read. Project files keep their paths relative to the
//! main file. Files from path and git dependencies are copied ("vendored") next
//! to the main file, because arXiv cannot fetch them. Files from the TeX
//! distribution are left out, because arXiv has its own. The generated `.bbl`
//! goes in too, because arXiv does not run Biber.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::compile::{self, BuildRequest};
use crate::diagnostics::{self, Reporter};
use crate::filelock;
use crate::manifest::Engine;
use crate::project::{Project, normalize, tex_path};
use crate::resolve::Resolution;
use crate::shell;

/// arXiv's usual limit for a submission's source files.
const SIZE_LIMIT: u64 = 50 * 1024 * 1024;
const IMAGE_EXTENSIONS: [&str; 5] = ["pdf", "png", "jpg", "jpeg", "eps"];
/// Environments whose contents are literal, so `%` is not a comment there.
const VERBATIM_ENVS: [&str; 6] = ["verbatim", "verbatim*", "Verbatim", "lstlisting", "minted", "alltt"];

pub struct PackRequest {
    pub target: String,
    pub profile: Option<String>,
    pub verify: bool,
    pub verbose: bool,
    pub reporter: Reporter,
}

pub fn pack(project: &Project, resolution: &Resolution, req: &PackRequest) -> Result<PathBuf> {
    if req.target != "arxiv" {
        bail!("unknown target `{}` (supported: arxiv)", req.target);
    }
    let doc = project.manifest.document.as_ref().ok_or_else(|| {
        anyhow!("`{}` has no [document] section, so there is nothing to pack", project.manifest.name())
    })?;
    let cfg = project.manifest.target.get("arxiv").cloned().unwrap_or_default();
    let profile_name = req.profile.clone().or(cfg.profile.clone()).unwrap_or_else(|| "release".into());
    let profile = project.manifest.profile(&profile_name)?;
    if profile.shell_escape {
        bail!(
            "profile `{}` enables shell-escape, which arXiv does not allow\n\
             help: for minted, use its `frozencache` option and ship the cache with `include` in [target.arxiv]",
            profile.name
        );
    }
    if doc.engine != Engine::Pdflatex {
        shell::warn(format!(
            "arXiv compiles most submissions with pdflatex; support for {} there may be limited",
            doc.engine
        ));
    }

    // 1. Build, so the recorder knows every file TeX read.
    compile::build(
        project,
        resolution,
        &BuildRequest { profile: profile.name.clone(), verbose: req.verbose, reporter: req.reporter },
    )?;

    let main = normalize(&project.root.join(&doc.main))?;
    let main_dir = main.parent().unwrap().to_path_buf();
    let main_name = main.file_name().unwrap().to_string_lossy().to_string();
    let main_stem = Path::new(&main_name).file_stem().unwrap().to_string_lossy().to_string();
    let out_dir = project.target_dir().join(&profile.name);
    let fls_path = out_dir.join(format!("{}.fls", doc.name));
    let fls = fs::read_to_string(&fls_path)
        .with_context(|| format!("failed to read the recorder file `{}`", project.display(&fls_path)))?;

    // 2. Decide which recorded files go into the bundle.
    let root = normalize(&project.root)?;
    let target_dir = root.join("target");
    let vendor_dirs = resolution.search_dirs();
    let personal = personal_tex_tree();
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    // Recorded files outside the project and its dependencies; checked below.
    let mut elsewhere = Vec::new();
    for input in parse_fls(&fls) {
        let Ok(path) = normalize(&input) else { continue };
        if path == main || path.starts_with(&target_dir) || path.is_dir() {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(&main_dir) {
            add_file(&mut files, tex_path(relative), path.clone())?;
        } else if vendor_dirs.iter().any(|d| path.starts_with(d)) {
            // Found through the search path, so TeX finds it by name next to the main file.
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            add_file(&mut files, name, path.clone())?;
        } else if path.starts_with(&root) {
            bail!(
                "`{}` is outside the main file's directory (`{}`)\n\
                 help: arXiv resolves paths from the main file's directory; move the file under it",
                project.display(&path),
                project.display(&main_dir)
            );
        } else if personal.as_ref().is_some_and(|p| path.starts_with(p)) {
            shell::warn(format!(
                "`{}` comes from your personal TeX tree, which arXiv does not have; \
                 make it a path dependency or add it to `include`",
                tex_path(&path)
            ));
        } else if has_source_extension(&path) {
            elsewhere.push(path);
        }
        // Fonts, maps and formats belong to the TeX distribution, which arXiv provides.
    }
    let missing = not_in_distribution(&elsewhere);
    if !missing.is_empty() {
        let list: Vec<String> = missing.iter().map(|p| format!("  {}", tex_path(p))).collect();
        bail!(
            "these files were read from outside the project, its dependencies and the TeX distribution, \
             so arXiv won't have them:\n{}\n\
             help: move them under the main file's directory, or make their folder a path dependency",
            list.join("\n")
        );
    }

    let main_source = fs::read_to_string(&main).with_context(|| format!("failed to read `{}`", main.display()))?;

    // Graphics the recorder may miss (xelatex and lualatex do not always record them).
    let mut tex_sources = vec![main_source.clone()];
    for (dest, src) in &files {
        if dest.ends_with(".tex") && src.starts_with(&main_dir) {
            tex_sources.push(fs::read_to_string(src).unwrap_or_default());
        }
    }
    for source in &tex_sources {
        for graphic in included_graphics(source) {
            if let Some(path) = find_graphic(&main_dir, &graphic) {
                let dest = tex_path(path.strip_prefix(&main_dir).unwrap());
                files.entry(dest).or_insert(path);
            }
        }
    }

    for extra in &cfg.include {
        let path = normalize(&project.root.join(extra)).with_context(|| format!("`include` entry `{extra}`"))?;
        let Ok(relative) = path.strip_prefix(&main_dir) else {
            bail!("`include` entry `{extra}` must be inside the main file's directory");
        };
        if path.is_dir() {
            for file in walk(&path)? {
                add_file(&mut files, tex_path(file.strip_prefix(&main_dir).unwrap()), file.clone())?;
            }
        } else {
            add_file(&mut files, tex_path(relative), path.clone())?;
        }
    }

    // 3. The main file: \pdfoutput first (arXiv looks at the first lines), then the
    //    profile's settings, then the document, optionally with \input files inlined.
    let mut body = main_source.clone();
    if cfg.flatten {
        let mut inlined = BTreeSet::new();
        body = flatten(&main_dir, &body, &mut inlined, 0)?;
        files.retain(|dest, _| !inlined.contains(dest));
    }
    let mut main_text = String::new();
    if doc.engine == Engine::Pdflatex {
        main_text.push_str("\\pdfoutput=1\n");
    }
    main_text.push_str(&compile::profile_prelude(&profile, &main_name, &main_source));
    main_text.push_str(&body);

    let strip = cfg.strip_comments.unwrap_or(true);
    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    entries.insert(main_name.clone(), maybe_strip(&main_text, strip).into_bytes());
    for (dest, src) in &files {
        let bytes = fs::read(src).with_context(|| format!("failed to read `{}`", src.display()))?;
        // Only the project's own .tex files are rewritten; vendored packages keep
        // their comments (often license text).
        let bytes = if strip && dest.ends_with(".tex") && src.starts_with(&main_dir) {
            maybe_strip(&String::from_utf8_lossy(&bytes), true).into_bytes()
        } else {
            bytes
        };
        entries.insert(dest.clone(), bytes);
    }

    // 4. The bibliography, already processed.
    let bbl = out_dir.join(format!("{}.bbl", doc.name));
    if let Ok(contents) = fs::read(&bbl) {
        if String::from_utf8_lossy(&contents).contains("biblatex") {
            shell::warn(
                "the bibliography was made with biblatex/Biber; arXiv only accepts the .bbl if your biblatex \
                 version matches the one in arXiv's TeX Live, so check it compiles there",
            );
        }
        entries.insert(format!("{main_stem}.bbl"), contents);
    } else if main_source.contains("\\bibliography{") || main_source.contains("\\addbibresource") {
        shell::warn("the document has a bibliography but no .bbl was produced; arXiv will not be able to build it");
    }

    check_names(&entries)?;
    let size: u64 = entries.values().map(|b| b.len() as u64).sum();
    if size > SIZE_LIMIT {
        shell::warn(format!("the bundle is {}, over arXiv's usual {} limit", human_size(size), human_size(SIZE_LIMIT)));
    }

    // 5. Write the bundle, check it compiles alone, and zip it.
    let pack_dir = project.target_dir().join("arxiv");
    let _lock = filelock::acquire(&pack_dir.join(".quire-lock"), "pack directory")?;
    let staging = pack_dir.join(&doc.name);
    write_tree(&staging, &entries)?;
    if req.verbose {
        for dest in entries.keys() {
            shell::status("Including", dest);
        }
    }

    if req.verify {
        verify(project, &pack_dir, &staging, &main_name, doc.engine, req)?;
    }

    let zip_path = pack_dir.join(format!("{}-arxiv.zip", doc.name));
    write_zip(&zip_path, &entries)?;
    let count = entries.len();
    let plural = if count == 1 { "" } else { "s" };
    shell::status("Packed", format!("{count} file{plural} ({}) → {}", human_size(size), project.display(&zip_path)));
    Ok(zip_path)
}

/// Inputs listed in a recorder (`.fls`) file, made absolute, in first-seen order.
pub fn parse_fls(fls: &str) -> Vec<PathBuf> {
    let mut pwd = PathBuf::new();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for line in fls.lines() {
        if let Some(dir) = line.strip_prefix("PWD ") {
            pwd = PathBuf::from(dir.trim());
        } else if let Some(file) = line.strip_prefix("INPUT ") {
            let file = PathBuf::from(file.trim());
            let absolute = if file.is_absolute() { file } else { pwd.join(file) };
            if seen.insert(absolute.clone()) {
                out.push(absolute);
            }
        }
    }
    out
}

fn add_file(files: &mut BTreeMap<String, PathBuf>, dest: String, src: PathBuf) -> Result<()> {
    if let Some(existing) = files.get(&dest)
        && *existing != src
    {
        bail!(
            "two files would both be packed as `{dest}`:\n  {}\n  {}\nhelp: rename one of them",
            existing.display(),
            src.display()
        );
    }
    files.insert(dest, src);
    Ok(())
}

/// The user's personal TeX tree (`TEXMFHOME`), if kpsewhich reports one.
fn personal_tex_tree() -> Option<PathBuf> {
    let out = Command::new("kpsewhich").arg("-var-value=TEXMFHOME").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    normalize(Path::new(&text)).ok()
}

/// Files a document author might have written or copied in (as opposed to
/// fonts, font maps and format files, which always come from the distribution).
fn has_source_extension(path: &Path) -> bool {
    const SOURCES: [&str; 14] =
        ["tex", "sty", "cls", "clo", "def", "cfg", "ldf", "bst", "bib", "pdf", "png", "jpg", "jpeg", "eps"];
    path.extension().is_some_and(|e| SOURCES.iter().any(|s| e.eq_ignore_ascii_case(s)))
}

/// Of `paths`, those the TeX distribution does not provide. A file counts as
/// provided when kpsewhich, run without quire's search path and outside the
/// project, resolves its name to that same file. If kpsewhich is unavailable,
/// nothing can be checked and nothing is reported.
fn not_in_distribution(paths: &[PathBuf]) -> Vec<PathBuf> {
    if paths.is_empty() {
        return Vec::new();
    }
    let Ok(neutral) = tempfile::tempdir() else { return Vec::new() };
    let mut cmd = Command::new("kpsewhich");
    cmd.current_dir(neutral.path()).env_remove("TEXINPUTS").env_remove("BIBINPUTS").env_remove("BSTINPUTS");
    for path in paths {
        cmd.arg(path.file_name().unwrap());
    }
    let Ok(out) = cmd.output() else { return Vec::new() };
    let found: BTreeSet<PathBuf> =
        String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| normalize(Path::new(l.trim())).ok()).collect();
    paths.iter().filter(|p| !found.contains(*p)).cloned().collect()
}

/// File names given to `\includegraphics`.
fn included_graphics(source: &str) -> Vec<String> {
    let text = compile::strip_comments(source);
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("\\includegraphics") {
        rest = &rest[i + "\\includegraphics".len()..];
        let mut tail = rest.trim_start().strip_prefix('*').unwrap_or(rest.trim_start()).trim_start();
        if tail.starts_with('[') {
            match tail.find(']') {
                Some(j) => tail = tail[j + 1..].trim_start(),
                None => continue,
            }
        }
        if let Some(arg) = tail.strip_prefix('{')
            && let Some(end) = arg.find('}')
        {
            out.push(arg[..end].trim().to_string());
        }
    }
    out
}

fn find_graphic(main_dir: &Path, name: &str) -> Option<PathBuf> {
    let base = main_dir.join(name);
    let candidates = std::iter::once(base.clone())
        .chain(IMAGE_EXTENSIONS.iter().map(|ext| base.with_extension(ext)))
        .chain(IMAGE_EXTENSIONS.iter().map(|ext| PathBuf::from(format!("{}.{ext}", base.display()))));
    candidates.filter(|p| p.is_file()).find_map(|p| normalize(&p).ok().filter(|p| p.starts_with(main_dir)))
}

fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read `{}`", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            out.extend(walk(&path)?);
        } else {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Index where code ends and a `%` comment starts (or the line length).
fn comment_start(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'%' => return i,
            _ => i += 1,
        }
    }
    line.len()
}

/// Inline `\input{...}` and `\include{...}` of files under the main file's
/// directory. Paths are relative to the main file's directory, as TeX sees them.
pub fn flatten(main_dir: &Path, source: &str, inlined: &mut BTreeSet<String>, depth: usize) -> Result<String> {
    if depth > 20 {
        bail!("\\input files are nested more than 20 levels deep (a cycle?)");
    }
    let mut out = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        let code_end = comment_start(line);
        let mut rest = line;
        let mut consumed = 0;
        loop {
            let found = ["\\input{", "\\include{"]
                .iter()
                .filter_map(|pat| rest.find(pat).map(|i| (i, *pat)))
                .min_by_key(|(i, _)| *i)
                .filter(|(i, _)| consumed + i < code_end);
            let Some((i, pat)) = found else { break };
            let arg_start = i + pat.len();
            let Some(len) = rest[arg_start..].find('}') else { break };
            let name = rest[arg_start..arg_start + len].trim();
            let Some((path, dest)) = resolve_input(main_dir, name) else {
                // Not a project file (e.g. from a package): leave the command alone.
                out.push_str(&rest[..arg_start + len + 1]);
                consumed += arg_start + len + 1;
                rest = &rest[arg_start + len + 1..];
                continue;
            };
            let content = fs::read_to_string(&path).with_context(|| format!("failed to read `{}`", path.display()))?;
            let flattened = flatten(main_dir, &content, inlined, depth + 1)?;
            // Drop the file's final newline so no blank line (= paragraph break)
            // appears, unless its last line ends in a comment that would swallow
            // the rest of this line.
            let mut content = flattened.strip_suffix('\n').unwrap_or(&flattened).to_string();
            let last_line = content.rsplit('\n').next().unwrap_or("");
            if comment_start(last_line) < last_line.len() {
                content.push('\n');
            }
            inlined.insert(dest);
            out.push_str(&rest[..i]);
            if pat == "\\include{" {
                out.push_str("\\clearpage\n");
                out.push_str(&content);
                out.push_str("\n\\clearpage");
            } else {
                out.push_str(&content);
            }
            consumed += arg_start + len + 1;
            rest = &rest[arg_start + len + 1..];
        }
        out.push_str(rest);
    }
    Ok(out)
}

fn resolve_input(main_dir: &Path, name: &str) -> Option<(PathBuf, String)> {
    let direct = main_dir.join(name);
    let path = if direct.is_file() { direct } else { main_dir.join(format!("{name}.tex")) };
    let path = normalize(&path).ok()?;
    let dest = tex_path(path.strip_prefix(main_dir).ok()?);
    Some((path, dest))
}

fn maybe_strip(text: &str, strip: bool) -> String {
    if strip { strip_tex_comments(text) } else { text.to_string() }
}

/// Remove comments the way arXiv's own cleaner does. A trailing `% ...` becomes
/// `%`, so the end-of-line space stays suppressed; lines that are only a comment
/// go away; `comment` environments are dropped. `\verb`, `\url`, `\href` and
/// verbatim-like environments keep their `%`.
pub fn strip_tex_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut verbatim_end: Option<String> = None;
    let mut in_comment_env = false;
    for line in source.split_inclusive('\n') {
        let (body, newline) = match line.strip_suffix('\n') {
            Some(b) => (b.strip_suffix('\r').unwrap_or(b), "\n"),
            None => (line, ""),
        };
        if in_comment_env {
            if body.contains("\\end{comment}") {
                in_comment_env = false;
            }
            continue;
        }
        if let Some(end) = &verbatim_end {
            if body.contains(end.as_str()) {
                verbatim_end = None;
            }
            out.push_str(body);
            out.push_str(newline);
            continue;
        }
        let trimmed = body.trim_start();
        if trimmed.starts_with("\\begin{comment}") {
            in_comment_env = !trimmed.contains("\\end{comment}");
            continue;
        }
        if trimmed.starts_with('%') {
            continue;
        }
        let stripped = strip_line(body);
        for env in VERBATIM_ENVS {
            if stripped.contains(&format!("\\begin{{{env}}}")) && !stripped.contains(&format!("\\end{{{env}}}")) {
                verbatim_end = Some(format!("\\end{{{env}}}"));
                // The rest of this line is literal too.
                out.push_str(body);
                out.push_str(newline);
                break;
            }
        }
        if verbatim_end.is_some() {
            continue;
        }
        out.push_str(&stripped);
        out.push_str(newline);
    }
    out
}

fn strip_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            let word: String = chars[i + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
            if word == "verb" {
                // \verb<d>...<d> or \verb*<d>...<d>: copy verbatim.
                let mut j = i + 5;
                if chars.get(j) == Some(&'*') {
                    j += 1;
                }
                if let Some(&delim) = chars.get(j) {
                    let end =
                        chars[j + 1..].iter().position(|&c| c == delim).map(|p| j + 1 + p + 1).unwrap_or(chars.len());
                    out.extend(&chars[i..end]);
                    i = end;
                    continue;
                }
            }
            if (word == "url" || word == "href") && chars.get(i + 1 + word.len()) == Some(&'{') {
                // Copy the braced argument, where % is literal.
                let start = i + 1 + word.len();
                let mut depth = 0;
                let mut end = chars.len();
                for (k, &ch) in chars.iter().enumerate().skip(start) {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = k + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                out.extend(&chars[i..end]);
                i = end;
                continue;
            }
            // Any other escape, including \% and \\.
            out.extend(&chars[i..(i + 2).min(chars.len())]);
            i += 2;
            continue;
        }
        if c == '%' {
            out.push('%');
            break;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// arXiv accepts only these characters in file names.
fn check_names(entries: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let bad: Vec<&String> = entries
        .keys()
        .filter(|name| !name.chars().all(|c| c.is_ascii_alphanumeric() || "_+-.,=/".contains(c)))
        .collect();
    if !bad.is_empty() {
        let list: Vec<String> = bad.iter().map(|n| format!("  {n}")).collect();
        bail!("arXiv only accepts letters, digits and `_ + - . , = /` in file names; rename:\n{}", list.join("\n"));
    }
    Ok(())
}

fn write_tree(dir: &Path, entries: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).with_context(|| format!("failed to clear `{}`", dir.display()))?;
    }
    for (dest, bytes) in entries {
        let path = dir.join(dest);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, bytes).with_context(|| format!("failed to write `{}`", path.display()))?;
    }
    Ok(())
}

/// Compile the bundle in a scratch copy with no access to the dependencies,
/// the way arXiv will: no search path, no Biber/BibTeX.
fn verify(
    project: &Project,
    pack_dir: &Path,
    staging: &Path,
    main_name: &str,
    engine: Engine,
    req: &PackRequest,
) -> Result<()> {
    let scratch = pack_dir.join("verify");
    let entries: BTreeMap<String, Vec<u8>> = walk(staging)?
        .into_iter()
        .map(|p| {
            let dest = tex_path(p.strip_prefix(staging).unwrap());
            fs::read(&p).map(|b| (dest, b))
        })
        .collect::<std::io::Result<_>>()?;
    write_tree(&scratch, &entries)?;

    shell::status("Verifying", "the bundle compiles on its own");
    let mut cmd = Command::new("latexmk");
    cmd.current_dir(&scratch)
        .arg(engine.latexmk_flag())
        .args(["-interaction=nonstopmode", "-halt-on-error", "-file-line-error", "-bibtex-"])
        .arg(main_name)
        .env_remove("TEXINPUTS")
        .env_remove("BIBINPUTS")
        .env_remove("BSTINPUTS");
    let output = cmd.output().context("failed to run latexmk")?;
    if !output.status.success() {
        let stem = Path::new(main_name).file_stem().unwrap().to_string_lossy().to_string();
        let log_path = scratch.join(format!("{stem}.log"));
        let log = fs::read_to_string(&log_path).unwrap_or_default();
        for mut error in diagnostics::parse_errors(&log).into_iter().take(5) {
            error.resolve_file(&scratch);
            req.reporter.diagnostic("arxiv-bundle", "arxiv", &error);
        }
        shell::note(format!("bundle log: {}", project.display(&log_path)));
        bail!(
            "the arXiv bundle does not compile on its own\n\
             help: a file is probably missing; add it with `include` in [target.arxiv], or use --no-verify to skip this check"
        );
    }
    Ok(())
}

/// A zip with sorted entries and fixed timestamps, so the same sources always
/// give the same bytes.
fn write_zip(path: &Path, entries: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let tmp = path.with_extension("zip.tmp");
    let file = File::create(&tmp).with_context(|| format!("failed to create `{}`", tmp.display()))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    for (dest, bytes) in entries {
        zip.start_file(dest.as_str(), options)?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;
    fs::rename(&tmp, path).with_context(|| format!("failed to write `{}`", path.display()))?;
    Ok(())
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_like_arxiv() {
        let src = r"\documentclass{article}
% a full-line comment
\newcommand\x{a}% trailing
50\% done % but this goes
\url{http://a.b/%20c} and \verb|%x| stay
\begin{comment}
secret
\end{comment}
\begin{verbatim}
% literal
\end{verbatim}
\\% after a line break
";
        let out = strip_tex_comments(src);
        assert_eq!(
            out,
            r"\documentclass{article}
\newcommand\x{a}%
50\% done %
\url{http://a.b/%20c} and \verb|%x| stay
\begin{verbatim}
% literal
\end{verbatim}
\\%
"
        );
    }

    #[test]
    fn parses_recorder_files() {
        let fls = "PWD /proj/src\nINPUT ./main.tex\nINPUT main.tex\nOUTPUT /proj/target/x.log\nINPUT /usr/share/a.sty\nINPUT /usr/share/a.sty\n";
        let inputs = parse_fls(fls);
        // `./main.tex` and `main.tex` are the same path; duplicates collapse.
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0], Path::new("/proj/src").join("main.tex"));
        assert!(inputs[1].ends_with("a.sty"));
    }

    #[test]
    fn finds_included_graphics() {
        let src = r"\includegraphics[width=\linewidth]{figs/plot}
% \includegraphics{commented}
\includegraphics*{logo.png}";
        assert_eq!(included_graphics(src), vec!["figs/plot", "logo.png"]);
    }

    #[test]
    fn flattens_inputs_and_includes() {
        let dir = tempfile::tempdir().unwrap();
        let main_dir = normalize(dir.path()).unwrap();
        fs::create_dir_all(main_dir.join("ch")).unwrap();
        fs::write(main_dir.join("ch/one.tex"), "One \\input{ch/two}\n").unwrap();
        fs::write(main_dir.join("ch/two.tex"), "Two").unwrap();
        fs::write(main_dir.join("ch/three.tex"), "Three\n").unwrap();
        let mut inlined = BTreeSet::new();
        let out = flatten(
            &main_dir,
            "A\n\\input{ch/one}\n% \\input{ch/two}\n\\include{ch/three}\n\\input{tikzlibrary}\n",
            &mut inlined,
            0,
        )
        .unwrap();
        assert_eq!(out, "A\nOne Two\n% \\input{ch/two}\n\\clearpage\nThree\n\\clearpage\n\\input{tikzlibrary}\n");
        assert_eq!(inlined.into_iter().collect::<Vec<_>>(), vec!["ch/one.tex", "ch/three.tex", "ch/two.tex"]);
    }

    #[test]
    fn rejects_bad_names() {
        let mut entries = BTreeMap::new();
        entries.insert("ok/fig-1.png".to_string(), vec![]);
        assert!(check_names(&entries).is_ok());
        entries.insert("my figure.png".to_string(), vec![]);
        assert!(check_names(&entries).unwrap_err().to_string().contains("my figure.png"));
    }
}
