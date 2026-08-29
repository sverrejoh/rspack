//! Content-based loader filter, evaluated before yielding to JS.
//!
//! A JS loader that can only act on modules containing a marker symbol
//! still costs one Rust->JS crossing for every module it is matched
//! against, because rule conditions are evaluated at factory time,
//! before content is read. `loader_should_yield` fires after content is
//! read and before the yield, so a content test here removes the
//! crossing outright for every non-matching module.
//!
//! Skipping is identity: declining the yield falls through to the
//! loader trait's no-op default (`JsLoader` never overrides `run()`),
//! so content, source map and additional data pass through unchanged.
//!
//! Configuration comes from `RSPACK_LOADER_CONTENT_FILTER`, a JSON
//! array. `loader` is matched as a substring of the loader request; a
//! module whose current content matches none of `include` (literal
//! needles) or `includeRegex` (regular expressions) is skipped for
//! that loader:
//!
//! ```json
//! [{"loader": "transform.mjs", "include": ["createServerFn"],
//!   "includeRegex": ["\\.\\s*handler\\s*\\("]}]
//! ```
//!
//! The same variable is read by the JS loader runner in
//! `@rspack/core`, which walks contiguous runs of JS loaders without
//! returning to Rust: a filter evaluated only here is unreachable for
//! every loader below the head of a run.
use std::sync::{
  OnceLock,
  atomic::{AtomicU64, Ordering},
};

use rspack_loader_runner::Content;

pub static CONTENT_FILTER_SKIPPED: AtomicU64 = AtomicU64::new(0);
pub static CONTENT_FILTER_CROSSED: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct ContentGate {
  loader: String,
  include: Vec<String>,
  include_regex: Vec<regex::bytes::Regex>,
}

fn gates() -> &'static Vec<ContentGate> {
  static GATES: OnceLock<Vec<ContentGate>> = OnceLock::new();
  GATES.get_or_init(|| {
    let Ok(raw) = std::env::var("RSPACK_LOADER_CONTENT_FILTER") else {
      return Vec::new();
    };
    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
      Ok(v) => v,
      Err(e) => {
        eprintln!("[rspack] invalid RSPACK_LOADER_CONTENT_FILTER: {e}");
        return Vec::new();
      }
    };
    let mut out = Vec::new();
    if let Some(items) = parsed.as_array() {
      for item in items {
        let loader = item
          .get("loader")
          .and_then(|v| v.as_str())
          .unwrap_or_default()
          .to_string();
        let include: Vec<String> = item
          .get("include")
          .and_then(|v| v.as_array())
          .map(|a| {
            a.iter()
              .filter_map(|m| m.as_str().map(|s| s.to_string()))
              .collect()
          })
          .unwrap_or_default();
        let mut include_regex = Vec::new();
        if let Some(list) = item.get("includeRegex").and_then(|v| v.as_array()) {
          for pattern in list.iter().filter_map(|p| p.as_str()) {
            match regex::bytes::Regex::new(pattern) {
              Ok(re) => include_regex.push(re),
              Err(e) => eprintln!("[rspack] invalid content filter regex {pattern:?}: {e}"),
            }
          }
        }
        if !loader.is_empty() && (!include.is_empty() || !include_regex.is_empty()) {
          out.push(ContentGate {
            loader,
            include,
            include_regex,
          });
        }
      }
    }
    out
  })
}

fn stats_enabled() -> bool {
  static ENABLED: OnceLock<bool> = OnceLock::new();
  *ENABLED.get_or_init(|| std::env::var("RSPACK_LOADER_CONTENT_FILTER_STATS").as_deref() == Ok("1"))
}

/// True when this loader is gated and the module content cannot match.
pub fn should_skip(loader_request: &str, content: Option<&Content>) -> bool {
  let gates = gates();
  if gates.is_empty() {
    return false;
  }
  let Some(gate) = gates.iter().find(|g| loader_request.contains(&g.loader)) else {
    return false;
  };
  let Some(content) = content else {
    return false;
  };
  // Marker search over raw bytes: never decodes, never allocates.
  let bytes = content.as_bytes();
  let matched = gate
    .include
    .iter()
    .any(|m| memchr::memmem::find(bytes, m.as_bytes()).is_some())
    || gate.include_regex.iter().any(|re| re.is_match(bytes));
  if matched {
    CONTENT_FILTER_CROSSED.fetch_add(1, Ordering::Relaxed);
    false
  } else {
    let n = CONTENT_FILTER_SKIPPED.fetch_add(1, Ordering::Relaxed) + 1;
    if stats_enabled() && n % 10_000 == 0 {
      eprintln!(
        "[rspack] content-filter(rust): skipped={} crossed={}",
        n,
        CONTENT_FILTER_CROSSED.load(Ordering::Relaxed)
      );
    }
    true
  }
}
