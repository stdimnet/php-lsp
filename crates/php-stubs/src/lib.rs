//! PHP core and extension stubs from
//! [JetBrains/phpstorm-stubs](https://github.com/JetBrains/phpstorm-stubs)
//! (Apache-2.0), embedded into the binary in compressed form.

use std::sync::OnceLock;

/// One stub file.
#[derive(Debug, Clone, Copy)]
pub struct StubFile {
    /// Extension directory, e.g. `standard`, `Core`, `redis`.
    pub extension: &'static str,
    /// Path relative to the stubs root, e.g. `standard/standard_0.php`.
    pub path: &'static str,
    pub text: &'static str,
}

static BLOB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/stubs.bin"));

/// The stub extensions enabled by default (same list as Intelephense's
/// `intelephense.stubs` default).
pub const DEFAULT_STUBS: &[&str] = &[
    "apache",
    "bcmath",
    "bz2",
    "calendar",
    "com_dotnet",
    "Core",
    "ctype",
    "curl",
    "date",
    "dba",
    "dom",
    "enchant",
    "exif",
    "FFI",
    "fileinfo",
    "filter",
    "fpm",
    "ftp",
    "gd",
    "gettext",
    "gmp",
    "hash",
    "iconv",
    "imap",
    "intl",
    "json",
    "ldap",
    "libxml",
    "mbstring",
    "meta",
    "mysqli",
    "oci8",
    "odbc",
    "openssl",
    "pcntl",
    "pcre",
    "PDO",
    "pdo_ibm",
    "pdo_mysql",
    "pdo_pgsql",
    "pdo_sqlite",
    "pgsql",
    "Phar",
    "posix",
    "pspell",
    "random",
    "readline",
    "Reflection",
    "session",
    "shmop",
    "SimpleXML",
    "snmp",
    "soap",
    "sockets",
    "sodium",
    "SPL",
    "sqlite3",
    "standard",
    "superglobals",
    "sysvmsg",
    "sysvsem",
    "sysvshm",
    "tidy",
    "tokenizer",
    "xml",
    "xmlreader",
    "xmlrpc",
    "xmlwriter",
    "xsl",
    "Zend OPcache",
    "zip",
    "zlib",
];

/// All embedded stub files (decompressed on first use).
pub fn all() -> &'static [StubFile] {
    static FILES: OnceLock<Vec<StubFile>> = OnceLock::new();
    FILES.get_or_init(|| {
        let raw = miniz_oxide::inflate::decompress_to_vec(BLOB).expect("corrupt stub blob");
        let raw: &'static [u8] = Box::leak(raw.into_boxed_slice());
        let mut pos = 0usize;
        let mut take = |n: usize| {
            let s = &raw[pos..pos + n];
            pos += n;
            s
        };
        let count = u32::from_le_bytes(take(4).try_into().unwrap()) as usize;
        let mut files = Vec::with_capacity(count);
        for _ in 0..count {
            let n = u16::from_le_bytes(take(2).try_into().unwrap()) as usize;
            let extension = std::str::from_utf8(take(n)).unwrap();
            let n = u16::from_le_bytes(take(2).try_into().unwrap()) as usize;
            let path = std::str::from_utf8(take(n)).unwrap();
            let n = u32::from_le_bytes(take(4).try_into().unwrap()) as usize;
            let text = std::str::from_utf8(take(n)).unwrap_or("");
            files.push(StubFile { extension, path, text });
        }
        files
    })
}

/// Names of all available stub extensions.
pub fn extensions() -> Vec<&'static str> {
    let mut exts: Vec<&str> = all().iter().map(|f| f.extension).collect();
    exts.dedup();
    exts
}

/// Stub files of the given extensions (case insensitive).
pub fn files_for<'a>(extensions: &'a [String]) -> impl Iterator<Item = &'static StubFile> + 'a {
    all().iter().filter(move |f| extensions.iter().any(|e| e.eq_ignore_ascii_case(f.extension)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn stubs_load() {
        let files = super::all();
        assert!(files.len() > 500);
        let standard = files.iter().find(|f| f.path == "standard/standard_0.php").unwrap();
        assert!(standard.text.contains("function"));
        for ext in ["Core", "standard", "SPL", "date", "pcre", "json", "PDO", "mbstring"] {
            assert!(super::extensions().contains(&ext), "missing stub extension {ext}");
        }
    }
}
