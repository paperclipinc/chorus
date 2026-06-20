//! Smoke test for the documented backend examples (issue #18).
//!
//! Every config under `examples/` is loaded through the same figment path the
//! server uses and validated. This keeps the docs honest: a config that no
//! longer parses or no longer passes `Config::validate` fails CI. A live test
//! against a running backend is out of scope here because it would need that
//! backend provisioned; this verifies the configs themselves.

use std::fs;
use std::path::PathBuf;

use chorus_core::config::Config;
use figment::Figment;
use figment::providers::{Format, Toml};

#[test]
fn example_configs_load_and_validate() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let mut checked = 0;
    for entry in fs::read_dir(&dir).expect("examples/ directory should exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let config: Config = Figment::new()
            .merge(Toml::file(&path))
            .extract()
            .unwrap_or_else(|e| panic!("example {} failed to parse: {e}", path.display()));
        config
            .validate()
            .unwrap_or_else(|e| panic!("example {} failed validation: {e}", path.display()));
        checked += 1;
    }
    assert!(
        checked >= 4,
        "expected at least the four documented backend examples, checked {checked}"
    );
}
