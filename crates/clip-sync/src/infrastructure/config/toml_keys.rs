//! Detect config keys that a TOML deserialize silently ignored.
//!
//! Config roots use `#[serde(flatten)]`, which is incompatible with
//! `#[serde(deny_unknown_fields)]`, so serde drops unknown / misspelled keys
//! without complaint — a typo then reads as "the setting had no effect". These
//! helpers recover the ignored keys so a caller can warn about them.
//!
//! Approach: round-trip the successfully-parsed config back to TOML and diff key
//! sets. Any key present in the user's text but absent from the re-serialization
//! was ignored by serde. The "known key" set is therefore always exactly what
//! the structs accept, with no hand-maintained list to drift. It is best-effort:
//! if the config cannot be re-serialized (or either side fails to re-parse) the
//! check yields nothing rather than guessing.
//!
//! Caveat: a field with `#[serde(skip_serializing_if = ...)]` that is skipped
//! would look "unknown" if the user set it. Config roots must not use
//! conditional skipping on accepted keys (verified by their roundtrip tests).

use serde::Serialize;

/// Return the dotted paths of keys present in the user's TOML `raw` but absent
/// from a re-serialization of the parsed `config` — i.e. the keys serde ignored.
///
/// Returns an empty vec if `config` cannot be re-serialized or either side fails
/// to re-parse as a table.
pub fn unknown_toml_keys<T: Serialize>(raw: &str, config: &T) -> Vec<String> {
    let Ok(known_str) = toml::to_string(config) else {
        return Vec::new();
    };
    let (Ok(original), Ok(known)) = (
        toml::from_str::<toml::Table>(raw),
        toml::from_str::<toml::Table>(&known_str),
    ) else {
        return Vec::new();
    };

    let mut unknown = Vec::new();
    collect_unknown_keys(&original, &known, "", &mut unknown);
    unknown
}

/// Recursively collect keys in `original` with no counterpart in `known`,
/// descending into tables that exist on both sides.
fn collect_unknown_keys(
    original: &toml::Table,
    known: &toml::Table,
    prefix: &str,
    out: &mut Vec<String>,
) {
    for (key, orig_val) in original {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match known.get(key) {
            None => out.push(path),
            Some(known_val) => {
                if let (Some(orig_tbl), Some(known_tbl)) =
                    (orig_val.as_table(), known_val.as_table())
                {
                    collect_unknown_keys(orig_tbl, known_tbl, &path, out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Default)]
    struct Inner {
        #[serde(default)]
        alpha: i64,
        #[serde(default)]
        beta: String,
    }

    #[derive(Serialize, Deserialize, Default)]
    struct Outer {
        #[serde(default)]
        top: i64,
        #[serde(default)]
        inner: Inner,
    }

    fn keys(raw: &str) -> Vec<String> {
        let config: Outer = toml::from_str(raw).expect("valid TOML for the test");
        unknown_toml_keys(raw, &config)
    }

    #[test]
    fn known_keys_are_not_flagged() {
        let raw = "top = 1\n[inner]\nalpha = 2\nbeta = \"x\"\n";
        assert!(keys(raw).is_empty(), "got {:?}", keys(raw));
    }

    #[test]
    fn unknown_top_level_key_is_flagged() {
        let raw = "topp = 1\n";
        assert_eq!(keys(raw), vec!["topp".to_string()]);
    }

    #[test]
    fn unknown_key_inside_known_table_is_flagged() {
        let raw = "[inner]\nalpa = 2\n";
        assert_eq!(keys(raw), vec!["inner.alpa".to_string()]);
    }

    #[test]
    fn entirely_unknown_table_is_flagged_at_its_root() {
        let raw = "[bogus]\nkey = 1\n";
        assert_eq!(keys(raw), vec!["bogus".to_string()]);
    }

    #[test]
    fn reports_only_the_bad_key_in_a_mixed_table() {
        let raw = "[inner]\nalpha = 1\ntypo = 2\n";
        assert_eq!(keys(raw), vec!["inner.typo".to_string()]);
    }
}
