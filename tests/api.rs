//! Integration tests exercising the public library API as an external consumer
//! would (issue #4): parse in-process without shelling out to the `orch` CLI.

use std::collections::HashMap;

use orch::error::OrchError;
use orch::types::{OrchFile, ServiceMode};

#[test]
fn parse_files_resolves_in_process() {
    let files = vec![(
        "Orchfile".to_string(),
        "SERVICE db\nFROM postgres:15\nPUBLISH 5432:5432\n".to_string(),
    )];
    let orch: OrchFile = orch::parse_files(&files, &HashMap::new()).expect("should parse");

    assert_eq!(orch.services.len(), 1);
    assert_eq!(orch.services[0].name, "db");
    assert_eq!(orch.services[0].mode, ServiceMode::Container);
    assert_eq!(orch.version, orch::types::ORCH_VERSION);
}

#[test]
fn parse_files_merges_overlays_left_to_right() {
    let files = vec![
        (
            "base.orch".to_string(),
            "SERVICE web\nFROM nginx\nPUBLISH 8080:80\n".to_string(),
        ),
        (
            "overlay.orch".to_string(),
            "SERVICE web\nPUBLISH 9090:80\n".to_string(),
        ),
    ];
    let orch = orch::parse_files(&files, &HashMap::new()).expect("should parse");

    assert_eq!(orch.services[0].publish.len(), 1);
    assert_eq!(orch.services[0].publish[0].host, 9090);
}

#[test]
fn parse_files_applies_overrides() {
    let files = vec![(
        "Orchfile".to_string(),
        "ARG tag=15\nSERVICE db\nFROM postgres:${tag}\n".to_string(),
    )];
    let mut overrides = HashMap::new();
    overrides.insert("tag".to_string(), "16".to_string());

    let orch = orch::parse_files(&files, &overrides).expect("should parse");
    assert_eq!(orch.services[0].image.as_deref(), Some("postgres:16"));
}

#[test]
fn parse_files_surfaces_structured_errors() {
    let files = vec![(
        "bad.orch".to_string(),
        "SERVICE bad\nFROM img\nRUN cmd\n".to_string(),
    )];
    let errs: Vec<OrchError> = orch::parse_files(&files, &HashMap::new()).expect_err("should fail");
    assert!(errs.iter().any(|e| e.to_string().contains("C1")));
}

#[test]
fn pipeline_modules_are_public_and_composable() {
    // The lower-level pipeline (parser -> merge -> resolve) is reachable directly.
    let raw = orch::parser::parse_raw("SERVICE x\nFROM img\n", 0).expect("raw parse");
    assert_eq!(raw.services.len(), 1);

    let merged = orch::merge::merge(vec![raw]);
    let resolved = orch::resolve::resolve(merged, &HashMap::new(), &[]).expect("resolve");
    assert_eq!(resolved.services[0].name, "x");
}
