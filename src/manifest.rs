//! `quire.toml`: parsing, validation and profile resolution.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub const MANIFEST_NAME: &str = "quire.toml";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    pub document: Option<Document>,
    pub package: Option<Package>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, RawDependency>,
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub profile: BTreeMap<String, ProfileConfig>,
    /// Packaging targets, e.g. `[target.arxiv]`.
    #[serde(default)]
    pub target: BTreeMap<String, TargetConfig>,
    /// Top-level keys quire does not understand; reported as warnings.
    #[serde(flatten)]
    pub unused: BTreeMap<String, toml::Value>,
}

/// A buildable document (like a Cargo binary target).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Document {
    pub name: String,
    #[serde(default = "default_main")]
    pub main: PathBuf,
    #[serde(default)]
    pub engine: Engine,
    /// TeX Live release to target. Recorded now, enforced once quire manages
    /// TeX Live packages itself.
    #[allow(dead_code)]
    pub texlive: Option<String>,
}

fn default_main() -> PathBuf {
    PathBuf::from("src/main.tex")
}

/// A reusable package (class, style files, macros) that other projects depend on
/// (like a Cargo library).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Package {
    pub name: String,
    #[allow(dead_code)] // used by `quire publish` (planned)
    pub version: String,
    #[allow(dead_code)]
    pub description: Option<String>,
    /// Directory exposed to TeX's search path, relative to the package root.
    #[serde(default = "default_tex_dir")]
    pub tex_dir: PathBuf,
}

fn default_tex_dir() -> PathBuf {
    PathBuf::from(".")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    Pdflatex,
    Xelatex,
    Lualatex,
}

impl Engine {
    pub fn as_str(self) -> &'static str {
        match self {
            Engine::Pdflatex => "pdflatex",
            Engine::Xelatex => "xelatex",
            Engine::Lualatex => "lualatex",
        }
    }

    pub fn latexmk_flag(self) -> &'static str {
        match self {
            Engine::Pdflatex => "-pdf",
            Engine::Xelatex => "-pdfxe",
            Engine::Lualatex => "-pdflua",
        }
    }
}

impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A dependency as written in `quire.toml`, before validation.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RawDependency {
    Version(String),
    Detailed(DetailedDependency),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DetailedDependency {
    pub version: Option<String>,
    pub path: Option<String>,
    pub git: Option<String>,
    pub tag: Option<String>,
    pub branch: Option<String>,
    pub rev: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// A validated dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSpec {
    /// Provided by the TeX distribution (TeX Live / MiKTeX).
    TexLive {
        version: String,
    },
    Path {
        path: PathBuf,
    },
    Git {
        url: String,
        reference: GitRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRef {
    DefaultBranch,
    Branch(String),
    Tag(String),
    Rev(String),
}

impl GitRef {
    /// Suffix used in lockfile source ids, e.g. `?tag=v1.0`.
    pub fn query(&self) -> String {
        match self {
            GitRef::DefaultBranch => String::new(),
            GitRef::Branch(b) => format!("?branch={b}"),
            GitRef::Tag(t) => format!("?tag={t}"),
            GitRef::Rev(r) => format!("?rev={r}"),
        }
    }
}

impl fmt::Display for GitRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitRef::DefaultBranch => f.write_str("the default branch"),
            GitRef::Branch(b) => write!(f, "branch `{b}`"),
            GitRef::Tag(t) => write!(f, "tag `{t}`"),
            GitRef::Rev(r) => write!(f, "revision `{r}`"),
        }
    }
}

impl RawDependency {
    pub fn to_spec(&self, name: &str) -> Result<DepSpec> {
        let d = match self {
            RawDependency::Version(v) => return Ok(DepSpec::TexLive { version: v.clone() }),
            RawDependency::Detailed(d) => d,
        };
        if let Some(key) = d.extra.keys().next() {
            bail!(
                "dependency `{name}` has unknown key `{key}` \
                 (expected one of: version, path, git, tag, branch, rev)"
            );
        }
        let refs = [&d.tag, &d.branch, &d.rev].iter().filter(|r| r.is_some()).count();
        match (&d.path, &d.git) {
            (Some(_), Some(_)) => bail!("dependency `{name}` specifies both `path` and `git`; pick one"),
            (Some(path), None) => {
                if refs > 0 {
                    bail!("dependency `{name}`: `tag`, `branch` and `rev` only apply to git dependencies");
                }
                Ok(DepSpec::Path { path: PathBuf::from(path) })
            }
            (None, Some(url)) => {
                if refs > 1 {
                    bail!("dependency `{name}`: specify at most one of `tag`, `branch` and `rev`");
                }
                let reference = if let Some(t) = &d.tag {
                    GitRef::Tag(t.clone())
                } else if let Some(b) = &d.branch {
                    GitRef::Branch(b.clone())
                } else if let Some(r) = &d.rev {
                    GitRef::Rev(r.clone())
                } else {
                    GitRef::DefaultBranch
                };
                Ok(DepSpec::Git { url: url.clone(), reference })
            }
            (None, None) => {
                if refs > 0 {
                    bail!("dependency `{name}`: `tag`, `branch` and `rev` require `git`");
                }
                match &d.version {
                    Some(v) => Ok(DepSpec::TexLive { version: v.clone() }),
                    None => bail!("dependency `{name}` needs a version, a `path` or a `git` URL"),
                }
            }
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct BuildConfig {
    #[serde(default)]
    pub shell_escape: bool,
    /// Extra arguments passed to latexmk for every profile.
    #[serde(default)]
    pub latexmk_args: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ProfileConfig {
    pub inherits: Option<String>,
    pub class_options: Option<Vec<String>>,
    pub pretex: Option<String>,
    pub synctex: Option<bool>,
    pub shell_escape: Option<bool>,
    pub latexmk_args: Option<Vec<String>>,
}

/// Settings for `quire pack --target <name>`.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TargetConfig {
    /// Build profile to pack (default: `release`).
    pub profile: Option<String>,
    /// Inline `\input`/`\include`d files into the main file.
    #[serde(default)]
    pub flatten: bool,
    /// Remove `%` comments and `comment` environments (default: true).
    pub strip_comments: Option<bool>,
    /// Extra files to ship, relative to the project root (e.g. data read by a package).
    #[serde(default)]
    pub include: Vec<String>,
}

pub const SUPPORTED_TARGETS: [&str; 1] = ["arxiv"];

/// A fully resolved build profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    /// Options passed to the document class (e.g. `draft`).
    pub class_options: Vec<String>,
    /// TeX code run before the main file is read.
    pub pretex: Option<String>,
    pub synctex: bool,
    pub shell_escape: bool,
    pub latexmk_args: Vec<String>,
}

pub const BUILTIN_PROFILES: [&str; 2] = ["dev", "release"];

impl Manifest {
    pub fn load(path: &Path) -> Result<Manifest> {
        let text = fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
        Manifest::parse(&text).with_context(|| format!("failed to parse `{}`", path.display()))
    }

    pub fn parse(text: &str) -> Result<Manifest> {
        let manifest: Manifest = toml::from_str(text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<()> {
        if self.document.is_none() && self.package.is_none() {
            bail!("a manifest needs a [document] section, a [package] section, or both");
        }
        if let Some(doc) = &self.document {
            validate_name(&doc.name).context("invalid `document.name`")?;
        }
        if let Some(pkg) = &self.package {
            validate_name(&pkg.name).context("invalid `package.name`")?;
        }
        for (name, dep) in &self.dependencies {
            validate_name(name).with_context(|| format!("invalid dependency name `{name}`"))?;
            dep.to_spec(name)?;
        }
        for name in self.target.keys() {
            if !SUPPORTED_TARGETS.contains(&name.as_str()) {
                bail!("unknown target `[target.{name}]` (supported: {})", SUPPORTED_TARGETS.join(", "));
            }
        }
        for name in self.profile.keys() {
            validate_name(name).with_context(|| format!("invalid profile name `{name}`"))?;
        }
        Ok(())
    }

    /// The project's display name.
    pub fn name(&self) -> &str {
        match (&self.document, &self.package) {
            (_, Some(pkg)) => &pkg.name,
            (Some(doc), None) => &doc.name,
            (None, None) => unreachable!("validated"),
        }
    }

    pub fn profile(&self, name: &str) -> Result<Profile> {
        self.profile_inner(name, 0)
    }

    fn profile_inner(&self, name: &str, depth: usize) -> Result<Profile> {
        if depth > 16 {
            bail!("profile `{name}` has an `inherits` cycle");
        }
        let user = self.profile.get(name);
        let mut profile = match name {
            "dev" | "release" => {
                if user.is_some_and(|u| u.inherits.is_some()) {
                    bail!("built-in profile `{name}` cannot use `inherits`");
                }
                Profile {
                    name: name.to_string(),
                    class_options: Vec::new(),
                    pretex: None,
                    synctex: name == "dev",
                    shell_escape: self.build.shell_escape,
                    latexmk_args: Vec::new(),
                }
            }
            _ => {
                let Some(user) = user else {
                    let mut defined: Vec<&str> = BUILTIN_PROFILES.to_vec();
                    defined.extend(self.profile.keys().map(String::as_str).filter(|k| !BUILTIN_PROFILES.contains(k)));
                    bail!("profile `{name}` is not defined (available: {})", defined.join(", "));
                };
                let parent = user.inherits.as_deref().unwrap_or("dev");
                if parent == name {
                    bail!("profile `{name}` inherits from itself");
                }
                self.profile_inner(parent, depth + 1).with_context(|| format!("in profile `{name}`"))?
            }
        };
        profile.name = name.to_string();
        if let Some(user) = user {
            if let Some(v) = &user.class_options {
                profile.class_options = v.clone();
            }
            if let Some(v) = &user.pretex {
                profile.pretex = Some(v.clone());
            }
            if let Some(v) = user.synctex {
                profile.synctex = v;
            }
            if let Some(v) = user.shell_escape {
                profile.shell_escape = v;
            }
            if let Some(v) = &user.latexmk_args {
                profile.latexmk_args = v.clone();
            }
        }
        Ok(profile)
    }
}

/// Names become TeX job names and directory names, so keep them simple.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("name cannot be empty");
    }
    if let Some(c) = name.chars().find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_')) {
        bail!("`{name}` contains `{c}`; only ASCII letters, digits, `-` and `_` are allowed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_manifest() {
        let m = Manifest::parse(
            r#"
            [document]
            name = "thesis"
            engine = "lualatex"
            texlive = "2025"

            [dependencies]
            siunitx = "*"
            lab = { path = "../lab" }
            uni = { git = "https://example.com/uni.git", tag = "v1.2" }

            [profile.draft]
            class-options = ["draft"]
            "#,
        )
        .unwrap();
        let doc = m.document.as_ref().unwrap();
        assert_eq!(doc.engine, Engine::Lualatex);
        assert_eq!(doc.main, PathBuf::from("src/main.tex"));
        assert_eq!(
            m.dependencies["uni"].to_spec("uni").unwrap(),
            DepSpec::Git { url: "https://example.com/uni.git".into(), reference: GitRef::Tag("v1.2".into()) }
        );
        assert_eq!(m.dependencies["lab"].to_spec("lab").unwrap(), DepSpec::Path { path: PathBuf::from("../lab") });
        assert_eq!(m.dependencies["siunitx"].to_spec("siunitx").unwrap(), DepSpec::TexLive { version: "*".into() });
    }

    #[test]
    fn rejects_bad_dependencies() {
        let err = |src: &str| Manifest::parse(src).unwrap_err().to_string();
        let base = "[document]\nname = \"x\"\n[dependencies]\n";
        assert!(err(&format!("{base}a = {{ path = \"p\", git = \"g\" }}")).contains("both"));
        assert!(err(&format!("{base}a = {{ git = \"g\", tag = \"t\", rev = \"r\" }}")).contains("at most one"));
        assert!(err(&format!("{base}a = {{ path = \"p\", tag = \"t\" }}")).contains("only apply"));
        assert!(err(&format!("{base}a = {{ paht = \"p\" }}")).contains("unknown key `paht`"));
        assert!(err("[build]\nshell-escape = true").contains("[document]"));
    }

    #[test]
    fn profiles_inherit_and_override() {
        let m = Manifest::parse(
            r#"
            [document]
            name = "x"
            [build]
            shell-escape = true
            [profile.release]
            latexmk-args = ["-gg"]
            [profile.draft]
            class-options = ["draft"]
            [profile.print]
            inherits = "release"
            class-options = ["twoside"]
            "#,
        )
        .unwrap();
        let dev = m.profile("dev").unwrap();
        assert!(dev.synctex && dev.shell_escape);
        let draft = m.profile("draft").unwrap();
        assert_eq!(draft.class_options, vec!["draft"]);
        assert!(draft.synctex);
        let print = m.profile("print").unwrap();
        assert!(!print.synctex);
        assert_eq!(print.latexmk_args, vec!["-gg"]);
        assert_eq!(print.class_options, vec!["twoside"]);
        assert!(m.profile("nope").unwrap_err().to_string().contains("available: dev, release, draft, print"));
    }

    #[test]
    fn detects_inherit_cycles() {
        let m =
            Manifest::parse("[document]\nname = \"x\"\n[profile.a]\ninherits = \"b\"\n[profile.b]\ninherits = \"a\"\n")
                .unwrap();
        assert!(format!("{:#}", m.profile("a").unwrap_err()).contains("cycle"));
    }
}
