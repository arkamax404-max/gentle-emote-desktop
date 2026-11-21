//! Shared appearance v1. No legacy imports, startup writes, or recovery/lock stealing.
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

pub const MAX_BYTES: usize = 65536;
/// Root object has depth 1; every nested object or array adds one.
pub const MAX_CONTAINER_DEPTH: usize = 64;
pub const MAX_INTEGER: i64 = 9_007_199_254_740_991;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn validate_agent_dir(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(invalid("agent directory must be absolute and nonempty"));
    }
    // components() discards interior '.' on Unix. Inspect the original OS
    // representation instead; ASCII separators are unambiguous even in WTF-8.
    if path
        .as_os_str()
        .as_encoded_bytes()
        .split(|b| *b == b'/' || (cfg!(windows) && *b == b'\\'))
        .any(|part| part == b"." || part == b"..")
    {
        return Err(invalid(
            "agent directory must not contain '.' or '..' components",
        ));
    }
    Ok(())
}

/// Pure resolver: explicit invalid values are errors, never fallback requests.
pub fn resolve_agent_dir(
    cli: Option<&OsStr>,
    env: Option<&OsStr>,
    home: Option<&OsStr>,
) -> io::Result<PathBuf> {
    let root = if let Some(root) = cli.or(env) {
        PathBuf::from(root)
    } else {
        let home = home.ok_or_else(|| {
            invalid("HOME is required when --agent-dir and PI_CODING_AGENT_DIR are absent")
        })?;
        if !Path::new(home).is_absolute() {
            return Err(invalid("HOME must be absolute and nonempty"));
        }
        PathBuf::from(home).join(".pi/agent")
    };
    validate_agent_dir(&root)?;
    Ok(root)
}

// A small bounded JSON tree preserves unknown values semantically. Integer-only
// parsing is intentional: accepting a floating-point intermediate loses lexemes
// and can round unknown nested values before their validation.
enum Json {
    Null,
    Bool(bool),
    Number(i64),
    // UTF-16 units keep escape/literal key equivalence; parsing validates pairs.
    String(Vec<u16>),
    Array(Vec<Json>),
    Object(BTreeMap<Vec<u16>, Json>),
}
fn key(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}
impl Json {
    fn object(&self) -> io::Result<&BTreeMap<Vec<u16>, Json>> {
        if let Self::Object(v) = self {
            Ok(v)
        } else {
            Err(invalid("expected JSON object"))
        }
    }
    fn encoded(&self) -> String {
        let mut out = String::new();
        self.encode(&mut out);
        out
    }
    fn encode(&self, out: &mut String) {
        enum Task<'a> {
            Value(&'a Json),
            Text(&'static str),
            String(&'a [u16]),
        }
        let mut tasks = vec![Task::Value(self)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::Text(s) => out.push_str(s),
                Task::String(s) => {
                    out.push('"');
                    for decoded in char::decode_utf16(s.iter().copied()) {
                        match decoded {
                            Ok('"') => out.push_str("\\\""),
                            Ok('\\') => out.push_str("\\\\"),
                            Ok(c) if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
                            Ok(c) => out.push(c),
                            Err(e) => out.push_str(&format!("\\u{:04x}", e.unpaired_surrogate())),
                        }
                    }
                    out.push('"');
                }
                Task::Value(value) => match value {
                    Self::Null => out.push_str("null"),
                    Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
                    Self::Number(n) => out.push_str(&n.to_string()),
                    Self::String(s) => tasks.push(Task::String(s)),
                    Self::Array(a) => {
                        out.push('[');
                        tasks.push(Task::Text("]"));
                        for (i, v) in a.iter().enumerate().rev() {
                            tasks.push(Task::Value(v));
                            if i > 0 {
                                tasks.push(Task::Text(","));
                            }
                        }
                    }
                    Self::Object(o) => {
                        out.push('{');
                        tasks.push(Task::Text("}"));
                        for (i, (k, v)) in o.iter().enumerate().rev() {
                            tasks.push(Task::Value(v));
                            tasks.push(Task::Text(":"));
                            tasks.push(Task::String(k));
                            if i > 0 {
                                tasks.push(Task::Text(","));
                            }
                        }
                    }
                },
            }
        }
    }
}
// Keep tree operations iterative, including destruction of partially parsed
// values rejected at the container-depth limit.
impl Drop for Json {
    fn drop(&mut self) {
        fn children(value: &mut Json, pending: &mut Vec<Json>) {
            match value {
                Json::Array(a) => pending.append(a),
                Json::Object(o) => pending.extend(std::mem::take(o).into_values()),
                _ => {}
            }
        }
        let mut pending = Vec::new();
        children(self, &mut pending);
        while let Some(mut value) = pending.pop() {
            children(&mut value, &mut pending);
        }
    }
}
impl Clone for Json {
    fn clone(&self) -> Self {
        let text = self.encoded();
        Parser {
            text: &text,
            pos: 0,
        }
        .value()
        .expect("validated JSON tree")
    }
}
impl PartialEq for Json {
    fn eq(&self, other: &Self) -> bool {
        self.encoded() == other.encoded()
    }
}
impl Eq for Json {}
impl std::fmt::Debug for Json {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.encoded())
    }
}
struct Parser<'a> {
    text: &'a str,
    pos: usize,
}
impl Parser<'_> {
    fn ws(&mut self) {
        while self
            .peek()
            .is_some_and(|b| matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.pos += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }
    fn take(&mut self, b: u8) -> io::Result<()> {
        if self.peek() != Some(b) {
            return Err(invalid("invalid JSON syntax"));
        }
        self.pos += 1;
        Ok(())
    }
    fn hex(&mut self) -> io::Result<u32> {
        let mut n = 0;
        for _ in 0..4 {
            let b = self
                .peek()
                .ok_or_else(|| invalid("incomplete Unicode escape"))?;
            let d = (b as char)
                .to_digit(16)
                .ok_or_else(|| invalid("invalid Unicode escape"))?;
            self.pos += 1;
            n = n * 16 + d;
        }
        Ok(n)
    }
    fn string(&mut self) -> io::Result<Vec<u16>> {
        self.take(b'"')?;
        let mut out = Vec::new();
        loop {
            let c = self.text[self.pos..]
                .chars()
                .next()
                .ok_or_else(|| invalid("unterminated JSON string"))?;
            self.pos += c.len_utf8();
            match c {
                '"' => {
                    if char::decode_utf16(out.iter().copied()).any(|unit| unit.is_err()) {
                        return Err(invalid("unpaired UTF-16 surrogate escape"));
                    }
                    return Ok(out);
                }
                '\\' => {
                    let b = self
                        .peek()
                        .ok_or_else(|| invalid("incomplete JSON escape"))?;
                    self.pos += 1;
                    out.push(match b {
                        b'"' => b'"' as u16,
                        b'\\' => b'\\' as u16,
                        b'/' => b'/' as u16,
                        b'b' => 8,
                        b'f' => 12,
                        b'n' => 10,
                        b'r' => 13,
                        b't' => 9,
                        b'u' => self.hex()? as u16,
                        _ => return Err(invalid("invalid JSON escape")),
                    });
                }
                c if c < ' ' => return Err(invalid("unescaped JSON control character")),
                c => out.extend(c.encode_utf16(&mut [0; 2]).iter().copied()),
            }
        }
    }
    fn value(&mut self) -> io::Result<Json> {
        enum Frame {
            Array(Vec<Json>),
            Object(BTreeMap<Vec<u16>, Json>, Vec<u16>),
        }
        let mut stack = Vec::new();
        loop {
            self.ws();
            // Frames are open ancestor containers. Check before entering even
            // an empty container: root depth is 1, and depth 65 is never entered.
            if matches!(self.peek(), Some(b'{' | b'[')) && stack.len() >= MAX_CONTAINER_DEPTH {
                return Err(invalid("JSON container depth exceeds 64"));
            }
            let mut value = match self.peek() {
                Some(b'{') => {
                    self.pos += 1;
                    self.ws();
                    if self.peek() == Some(b'}') {
                        self.pos += 1;
                        Json::Object(BTreeMap::new())
                    } else {
                        let key = self.string()?;
                        self.ws();
                        self.take(b':')?;
                        stack.push(Frame::Object(BTreeMap::new(), key));
                        continue;
                    }
                }
                Some(b'[') => {
                    self.pos += 1;
                    self.ws();
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        Json::Array(Vec::new())
                    } else {
                        stack.push(Frame::Array(Vec::new()));
                        continue;
                    }
                }
                _ => self.scalar()?,
            };
            loop {
                let Some(frame) = stack.last_mut() else {
                    return Ok(value);
                };
                self.ws();
                match frame {
                    Frame::Array(a) => {
                        a.push(value);
                        if self.peek() == Some(b',') {
                            self.pos += 1;
                            break;
                        }
                        self.take(b']')?;
                    }
                    Frame::Object(o, key) => {
                        if o.insert(std::mem::take(key), value).is_some() {
                            return Err(invalid("duplicate JSON key"));
                        }
                        if self.peek() == Some(b',') {
                            self.pos += 1;
                            self.ws();
                            *key = self.string()?;
                            self.ws();
                            self.take(b':')?;
                            break;
                        }
                        self.take(b'}')?;
                    }
                }
                value = match stack.pop().expect("current frame") {
                    Frame::Array(a) => Json::Array(a),
                    Frame::Object(o, _) => Json::Object(o),
                };
            }
        }
    }
    fn scalar(&mut self) -> io::Result<Json> {
        match self.peek() {
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.pos;
                if self.peek() == Some(b'-') {
                    self.pos += 1;
                }
                if self.peek() == Some(b'0') {
                    self.pos += 1;
                } else {
                    let digits = self.pos;
                    while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                        self.pos += 1;
                    }
                    if self.pos == digits {
                        return Err(invalid("invalid JSON integer"));
                    }
                }
                if self
                    .peek()
                    .is_some_and(|b| matches!(b, b'.' | b'e' | b'E') || b.is_ascii_digit())
                {
                    return Err(invalid("all JSON numbers must use integer lexemes"));
                }
                let n = self.text[start..self.pos]
                    .parse::<i64>()
                    .map_err(|_| invalid("JSON integer outside JS-safe range"))?;
                if !(-MAX_INTEGER..=MAX_INTEGER).contains(&n) {
                    return Err(invalid("JSON integer outside JS-safe range"));
                }
                Ok(Json::Number(n))
            }
            _ => {
                for (word, value) in [
                    ("true", Json::Bool(true)),
                    ("false", Json::Bool(false)),
                    ("null", Json::Null),
                ] {
                    if self.text[self.pos..].starts_with(word) {
                        self.pos += word.len();
                        return Ok(value);
                    }
                }
                Err(invalid("invalid JSON value"))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub revision: u64,
    pub rgb: [u8; 3],
    pub opacity: u8,
    pub transparent: bool,
    document: Json,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self::parse(br#"{"version":1,"revision":0,"desktop":{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true}}"#)
            .expect("valid built-in defaults")
    }
}
impl Snapshot {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.starts_with(b"\xef\xbb\xbf") {
            return Err(invalid("appearance JSON must not contain a UTF-8 BOM"));
        }
        if bytes.len() > MAX_BYTES {
            return Err(invalid("appearance JSON exceeds 65536 bytes"));
        }
        let text =
            std::str::from_utf8(bytes).map_err(|_| invalid("appearance JSON must be UTF-8"))?;
        let mut p = Parser { text, pos: 0 };
        let document = p.value()?;
        p.ws();
        if p.pos != text.len() {
            return Err(invalid("trailing JSON content"));
        }
        Self::from_document(document)
    }
    fn from_document(document: Json) -> io::Result<Self> {
        let obj = document.object()?;
        if obj.get(&key("version")) != Some(&Json::Number(1)) {
            return Err(invalid("unsupported appearance version"));
        }
        let Some(Json::Number(revision)) = obj.get(&key("revision")) else {
            return Err(invalid("revision must be an unsigned JS-safe integer"));
        };
        if *revision < 0 {
            return Err(invalid("revision must be an unsigned JS-safe integer"));
        }
        let desktop = obj
            .get(&key("desktop"))
            .ok_or_else(|| invalid("desktop is required"))?
            .object()?;
        let Some(Json::Array(rgb)) = desktop.get(&key("backgroundRgb")) else {
            return Err(invalid("backgroundRgb must contain three bytes"));
        };
        if rgb.len() != 3 {
            return Err(invalid("backgroundRgb must contain three bytes"));
        }
        let mut color = [0; 3];
        for (c, v) in color.iter_mut().zip(rgb) {
            let Json::Number(n) = v else {
                return Err(invalid("backgroundRgb must contain three bytes"));
            };
            *c = u8::try_from(*n).map_err(|_| invalid("backgroundRgb must contain three bytes"))?;
        }
        let opacity = match desktop.get(&key("backgroundOpacity")) {
            Some(Json::Number(n)) if (0..=100).contains(n) => *n as u8,
            _ => return Err(invalid("backgroundOpacity must be 0..100")),
        };
        let transparent = match desktop.get(&key("transparentBackground")) {
            Some(Json::Bool(b)) => *b,
            _ => return Err(invalid("transparentBackground must be boolean")),
        };
        if !transparent && opacity != 100 {
            return Err(invalid("disabled transparency requires opacity 100"));
        }
        Ok(Self {
            revision: *revision as u64,
            rgb: color,
            opacity,
            transparent,
            document,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Patch {
    Rgb([u8; 3]),
    Transparency { opacity: u8, enabled: bool },
}
#[derive(Debug)]
pub enum SaveResult {
    Committed(Snapshot),
    Conflict(Snapshot),
    /// Rename committed; callers must not retry this patch.
    CommittedDurabilityUncertain(Snapshot, String),
    /// Compatibility API: retain the original outcome and its snapshot.
    WithCleanupError(Box<SaveResult>, String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ready,
    Missing,
    Saved,
    Conflict,
    RevisionExhausted,
    Busy,
    Invalid,
    UnsafePath,
    IoError,
    UnsupportedPlatform,
    CommittedDurabilityUncertain,
}
impl Status {
    pub fn code(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Missing => "missing",
            Self::Saved => "saved",
            Self::Conflict => "conflict",
            Self::RevisionExhausted => "revision-exhausted",
            Self::Busy => "busy",
            Self::Invalid => "invalid",
            Self::UnsafePath => "unsafe-path",
            Self::IoError => "io-error",
            Self::UnsupportedPlatform => "unsupported-platform",
            Self::CommittedDurabilityUncertain => "committed-durability-uncertain",
        }
    }
    pub fn from_error(error: &io::Error) -> Self {
        if let Some(tag) = error
            .get_ref()
            .and_then(|e| e.downcast_ref::<StatusError>())
        {
            return tag.status;
        }
        match error.kind() {
            io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => Self::Invalid,
            io::ErrorKind::TimedOut => Self::Busy,
            io::ErrorKind::Unsupported => Self::UnsupportedPlatform,
            _ => Self::IoError,
        }
    }
}
#[derive(Debug)]
struct StatusError {
    status: Status,
    message: String,
}
impl std::fmt::Display for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for StatusError {}
fn status_error(status: Status, kind: io::ErrorKind, message: impl Into<String>) -> io::Error {
    io::Error::new(
        kind,
        StatusError {
            status,
            message: message.into(),
        },
    )
}

/// Primary status/snapshot never changes because a separate cleanup failed.
#[derive(Debug)]
pub struct Completion {
    pub status: Status,
    pub snapshot: Option<Snapshot>,
    pub error: Option<io::Error>,
    pub cleanup_error: Option<String>,
}
impl Completion {
    fn snapshot(status: Status, snapshot: Snapshot) -> Self {
        Self {
            status,
            snapshot: Some(snapshot),
            error: None,
            cleanup_error: None,
        }
    }
    fn failed(error: io::Error) -> Self {
        Self {
            status: Status::from_error(&error),
            snapshot: None,
            error: Some(error),
            cleanup_error: None,
        }
    }
    pub fn warning(&self) -> Option<String> {
        let mut warnings = Vec::new();
        match self.status {
            Status::Conflict => warnings.push(
                "Appearance conflict: refreshed current settings; edit again to save (no retry)."
                    .into(),
            ),
            Status::CommittedDurabilityUncertain => warnings.push(format!(
                "Appearance committed, durability uncertain: {}; do not retry.",
                self.error
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default()
            )),
            _ => {
                if let Some(error) = &self.error {
                    warnings.push(format!("Appearance error: {error}"));
                }
            }
        }
        if let Some(error) = &self.cleanup_error {
            warnings.push(format!("Appearance cleanupError: {error}"));
        }
        (!warnings.is_empty()).then(|| warnings.join("\n"))
    }
    fn into_error(self) -> io::Error {
        let error = self
            .error
            .unwrap_or_else(|| io::Error::other(self.status.code()));
        if let Some(cleanup) = self.cleanup_error {
            status_error(
                self.status,
                error.kind(),
                format!("{error}; cleanupError: {cleanup}"),
            )
        } else {
            error
        }
    }
}
#[derive(Clone, Debug)]
pub struct Store {
    agent_dir: PathBuf,
    path: PathBuf,
}
impl Store {
    pub fn new(agent_dir: PathBuf) -> io::Result<Self> {
        validate_agent_dir(&agent_dir)?;
        Ok(Self {
            path: agent_dir.join("extensions/gentle-emote/appearance.json"),
            agent_dir,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn load_report(&self) -> Completion {
        match platform::load(self) {
            Ok((snapshot, missing)) => Completion::snapshot(
                if missing {
                    Status::Missing
                } else {
                    Status::Ready
                },
                snapshot,
            ),
            Err(error) => Completion::failed(error),
        }
    }
    pub fn patch_report(&self, expected_revision: u64, patch: Patch) -> Completion {
        if let Patch::Transparency { opacity, enabled } = patch
            && (opacity > 100 || (!enabled && opacity != 100))
        {
            return Completion::failed(invalid("invalid transparency patch"));
        }
        platform::patch(self, expected_revision, patch)
    }
    pub fn load(&self) -> io::Result<Snapshot> {
        let report = self.load_report();
        if report.error.is_some() {
            Err(report.into_error())
        } else {
            Ok(report.snapshot.expect("successful read snapshot"))
        }
    }
    pub fn patch(&self, expected_revision: u64, patch: Patch) -> io::Result<SaveResult> {
        let mut report = self.patch_report(expected_revision, patch);
        let outcome = match report.status {
            Status::Saved => SaveResult::Committed(report.snapshot.take().expect("saved snapshot")),
            Status::Conflict => {
                SaveResult::Conflict(report.snapshot.take().expect("conflict snapshot"))
            }
            Status::CommittedDurabilityUncertain => SaveResult::CommittedDurabilityUncertain(
                report.snapshot.take().expect("committed snapshot"),
                report.error.take().expect("durability error").to_string(),
            ),
            _ => return Err(report.into_error()),
        };
        Ok(if let Some(error) = report.cleanup_error {
            SaveResult::WithCleanupError(Box::new(outcome), error)
        } else {
            outcome
        })
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::{
        fs::{self, File, Metadata, OpenOptions},
        io::{Read, Write},
        os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        time::{Duration, Instant},
    };
    fn unsafe_path(message: &str) -> io::Error {
        status_error(Status::UnsafePath, io::ErrorKind::PermissionDenied, message)
    }
    fn owned(meta: &Metadata, directory: bool) -> io::Result<()> {
        // SAFETY: geteuid has no arguments or memory preconditions.
        let uid = unsafe { libc::geteuid() };
        if meta.uid() != uid
            || meta.mode() & 0o022 != 0
            || if directory {
                !meta.is_dir()
            } else {
                !meta.is_file()
            }
        {
            return Err(unsafe_path(
                "appearance path must be same-user, not group/other writable, and the expected type",
            ));
        }
        Ok(())
    }
    fn metadata(path: &Path, directory: bool) -> io::Result<Metadata> {
        let m = fs::symlink_metadata(path)?;
        owned(&m, directory)?;
        Ok(m)
    }
    // lstat every prefix. Only agentDir and its descendants require our UID and
    // no group/other writes; system ancestors may be root-owned (or /tmp).
    fn walk_parent(store: &Store, create: bool) -> io::Result<bool> {
        let parent = store.path.parent().expect("constructed store parent");
        let mut prefix = PathBuf::new();
        for component in parent.components() {
            prefix.push(component.as_os_str());
            let meta = match fs::symlink_metadata(&prefix) {
                Ok(m) => m,
                Err(e) if e.kind() == io::ErrorKind::NotFound && create => {
                    match fs::DirBuilder::new().mode(0o700).create(&prefix) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(e),
                    }
                    fs::symlink_metadata(&prefix)?
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(e) => return Err(e),
            };
            if !meta.is_dir() {
                return Err(unsafe_path(
                    "appearance ancestor is a symlink or non-directory",
                ));
            }
            if prefix.starts_with(&store.agent_dir) {
                owned(&meta, true)?;
            }
        }
        Ok(true)
    }
    fn same(a: &Metadata, b: &Metadata) -> bool {
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    fn read_identified(path: &Path) -> io::Result<(Vec<u8>, Metadata)> {
        let before = metadata(path, false)?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let after = file.metadata()?;
        owned(&after, false)?;
        if !same(&before, &after) {
            return Err(unsafe_path("appearance file identity changed during open"));
        }
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
        Ok((bytes, after))
    }
    fn read(path: &Path) -> io::Result<Vec<u8>> {
        read_identified(path).map(|(bytes, _)| bytes)
    }
    fn source(store: &Store) -> io::Result<(Snapshot, Option<Metadata>)> {
        if !walk_parent(store, false)? {
            return Ok((Snapshot::default(), None));
        }
        match read_identified(&store.path) {
            Ok((bytes, identity)) => Ok((Snapshot::parse(&bytes)?, Some(identity))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok((Snapshot::default(), None)),
            Err(e) => Err(e),
        }
    }
    pub(super) fn load(store: &Store) -> io::Result<(Snapshot, bool)> {
        source(store).map(|(snapshot, identity)| (snapshot, identity.is_none()))
    }
    fn token() -> io::Result<String> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
        Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }
    fn create(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
    }
    struct OwnedFile {
        path: PathBuf,
        identity: Metadata,
    }
    impl OwnedFile {
        fn create(path: PathBuf, notes: &mut Vec<String>) -> io::Result<(Self, File)> {
            let file = create(&path).map_err(|e| {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    unsafe_path("appearance exclusive-create path already exists")
                } else {
                    e
                }
            })?;
            let identity = file.metadata().inspect_err(|_| {
                notes.push(format!(
                    "{}: identity unavailable after create; manual recovery required",
                    path.display()
                ))
            })?;
            Ok((Self { path, identity }, file))
        }
        fn check(&self) -> io::Result<()> {
            let current = metadata(&self.path, false)?;
            if !same(&current, &self.identity) {
                return Err(unsafe_path("owned appearance file identity changed"));
            }
            Ok(())
        }
        fn remove(&self) -> io::Result<()> {
            self.check()?;
            fs::remove_file(&self.path)
        }
    }
    struct Lock {
        path: PathBuf,
        identity: Metadata,
        marker: Option<OwnedFile>,
        token: String,
        complete: bool,
    }
    impl Lock {
        fn check_directory(&self) -> io::Result<()> {
            let current = metadata(&self.path, true)?;
            if !same(&current, &self.identity) {
                return Err(unsafe_path("appearance lock directory identity changed"));
            }
            Ok(())
        }
        fn check(&self) -> io::Result<()> {
            self.check_directory()?;
            if let Some(marker) = &self.marker {
                marker.check()?;
                if self.complete && read(&marker.path)? != format!("v1:{}\n", self.token).as_bytes()
                {
                    return Err(unsafe_path("appearance lock owner token changed"));
                }
            } else if self.complete {
                return Err(unsafe_path("appearance lock marker missing"));
            }
            Ok(())
        }
        fn remove(&mut self) -> io::Result<()> {
            self.check()?;
            if let Some(marker) = self.marker.take() {
                marker.remove()?;
            }
            self.check_directory()?;
            fs::remove_dir(&self.path)
        }
    }
    // Normal exits explicitly collect errors. Drop is only an unwind backstop;
    // taking each guard first prevents hidden cleanup retries after a failure.
    struct Transaction<'a> {
        store: &'a Store,
        lock: Option<Lock>,
        temp: Option<OwnedFile>,
        notes: Vec<String>,
    }
    impl Transaction<'_> {
        fn acquire(&mut self) -> io::Result<()> {
            let path = self.store.path.with_file_name("appearance.json.lock");
            let start = Instant::now();
            let token = token()?;
            loop {
                if start.elapsed() >= Duration::from_millis(1000) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "appearance lock timeout; manual recovery required, never stolen",
                    ));
                }
                if !walk_parent(self.store, false)? {
                    return Err(unsafe_path("appearance parent disappeared"));
                }
                match fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => {
                        let identity = fs::symlink_metadata(&path).inspect_err(|_| {
                            self.notes.push(format!(
                                "{}: identity unavailable after mkdir; manual recovery required",
                                path.display()
                            ))
                        })?;
                        self.lock = Some(Lock {
                            path,
                            identity,
                            marker: None,
                            token,
                            complete: false,
                        });
                        let lock = self.lock.as_mut().expect("created lock");
                        lock.check_directory()?;
                        let (marker, mut file) =
                            OwnedFile::create(lock.path.join("owner"), &mut self.notes)?;
                        lock.marker = Some(marker);
                        file.write_all(format!("v1:{}\n", lock.token).as_bytes())?;
                        file.flush()?;
                        file.sync_all()?;
                        drop(file);
                        lock.complete = true;
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        match metadata(&path, true) {
                            Ok(_) => {}
                            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                            Err(e) => return Err(e),
                        }
                        // Never use a malformed/partial marker as permission to
                        // steal. Still reject an unsafe existing marker leaf.
                        match metadata(&path.join("owner"), false) {
                            Ok(_) => {}
                            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                            Err(e) => return Err(e),
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        fn cleanup(&mut self) -> Option<String> {
            if let Some(temp) = self.temp.take() {
                let result = walk_parent(self.store, false).and_then(|exists| {
                    if exists {
                        temp.remove()
                    } else {
                        Err(unsafe_path(
                            "appearance parent disappeared during temp cleanup",
                        ))
                    }
                });
                if let Err(e) = result {
                    self.notes.push(format!("temp: {e}"));
                }
            }
            // Independent of the temp result, always attempt owned lock cleanup.
            if let Some(mut lock) = self.lock.take() {
                let result = walk_parent(self.store, false).and_then(|exists| {
                    if exists {
                        lock.remove()
                    } else {
                        Err(unsafe_path(
                            "appearance parent disappeared during lock cleanup",
                        ))
                    }
                });
                if let Err(e) = result {
                    self.notes.push(format!("lock: {e}"));
                }
            }
            let notes = std::mem::take(&mut self.notes);
            (!notes.is_empty()).then(|| notes.join("; "))
        }
        fn save(&mut self, expected: u64, patch: Patch) -> io::Result<Completion> {
            walk_parent(self.store, true)?;
            self.acquire()?;
            let (mut current, identity) = source(self.store)?;
            if current.revision != expected {
                return Ok(Completion::snapshot(Status::Conflict, current));
            }
            if current.revision >= MAX_INTEGER as u64 {
                return Err(status_error(
                    Status::RevisionExhausted,
                    io::ErrorKind::InvalidData,
                    "appearance revision overflow",
                ));
            }
            let Json::Object(ref mut doc) = current.document else {
                unreachable!()
            };
            let Some(Json::Object(desktop)) = doc.get_mut(&key("desktop")) else {
                unreachable!()
            };
            // Input is depth-validated. These patches add no deeper structure:
            // RGB is always at depth 3; opacity/checkbox are desktop scalars.
            match patch {
                Patch::Rgb(rgb) => {
                    desktop.insert(
                        key("backgroundRgb"),
                        Json::Array(rgb.into_iter().map(|n| Json::Number(n.into())).collect()),
                    );
                }
                Patch::Transparency { opacity, enabled } => {
                    desktop.insert(key("backgroundOpacity"), Json::Number(opacity.into()));
                    desktop.insert(key("transparentBackground"), Json::Bool(enabled));
                }
            }
            doc.insert(key("revision"), Json::Number((current.revision + 1) as i64));
            let confirmed = Snapshot::from_document(current.document)?;
            let mut bytes = String::new();
            confirmed.document.encode(&mut bytes);
            bytes.push('\n');
            if bytes.len() > MAX_BYTES {
                return Err(invalid("patched appearance JSON exceeds 65536 bytes"));
            }
            let lock = self.lock.as_ref().expect("owned lock");
            let temp_path = self
                .store
                .path
                .with_file_name(format!("appearance.json.{}.tmp", lock.token));
            let (temp, mut file) = OwnedFile::create(temp_path, &mut self.notes)?;
            self.temp = Some(temp);
            file.write_all(bytes.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            if !walk_parent(self.store, false)? {
                return Err(unsafe_path("appearance parent disappeared before commit"));
            }
            self.lock.as_ref().expect("owned lock").check()?;
            let temp = self.temp.as_ref().expect("owned temp");
            temp.check()?;
            let destination = match metadata(&self.store.path, false) {
                Ok(m) => Some(m),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            };
            let unchanged = match (&identity, &destination) {
                (Some(a), Some(b)) => same(a, b),
                (None, None) => true,
                _ => false,
            };
            if !unchanged {
                return Err(unsafe_path(
                    "appearance source identity changed before commit",
                ));
            }
            fs::rename(&temp.path, &self.store.path)?; // Sole commit point; never delete a destination.
            self.temp.take(); // The path was consumed by our successful rename.
            let mut completion = Completion::snapshot(Status::Saved, confirmed);
            if let Err(error) = sync_parent(self.store.path.parent().expect("constructed parent")) {
                completion.status = Status::CommittedDurabilityUncertain;
                completion.error = Some(error);
            }
            Ok(completion)
        }
    }
    impl Drop for Transaction<'_> {
        fn drop(&mut self) {
            let _ = self.cleanup();
        }
    }
    fn sync_parent(parent: &Path) -> io::Result<()> {
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(parent)?;
        match directory.sync_all() {
            // These codes specifically mean directory fsync is not implemented
            // on the open directory's filesystem, not that a commit failed.
            Err(e)
                if e.kind() == io::ErrorKind::Unsupported
                    || e.raw_os_error().is_some_and(|code| {
                        code == libc::EINVAL || code == libc::ENOTSUP || code == libc::EOPNOTSUPP
                    }) =>
            {
                Ok(())
            }
            result => result,
        }
    }
    pub(super) fn patch(store: &Store, expected: u64, patch: Patch) -> Completion {
        let mut transaction = Transaction {
            store,
            lock: None,
            temp: None,
            notes: Vec::new(),
        };
        let mut completion = match transaction.save(expected, patch) {
            Ok(report) => report,
            Err(error) => Completion::failed(error),
        };
        completion.cleanup_error = transaction.cleanup();
        completion
    }
}
#[cfg(not(unix))]
mod platform {
    use super::*;
    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "shared appearance persistence unsupported: Windows ACL contract awaits owner clarification",
        )
    }
    pub(super) fn load(store: &Store) -> io::Result<(Snapshot, bool)> {
        debug_assert!(store.agent_dir.is_absolute());
        Err(unsupported())
    }
    pub(super) fn patch(_: &Store, _: u64, _: Patch) -> Completion {
        Completion::failed(unsupported())
    }
}
