// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostic: open an INI + .msq the way `open_tune` does and report what
//! failed to apply.
//! Usage: cargo run -p opentune-project --example msq_dump -- <ini> <msq> [SYMBOL,SYMBOL,...]
//!
//! The optional symbol list seeds the preprocessor's `#if` gates — pass the
//! project's `project.properties` → `ecuSettings` values to reproduce what
//! the app resolves (it reads them from the sibling file automatically).

use std::sync::Arc;

fn read_text(path: &str) -> String {
    let bytes = std::fs::read(path).expect("read file");
    String::from_utf8(bytes).unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let ini_path = args
        .next()
        .expect("usage: msq_dump <ini> <msq> [SYMBOL,...]");
    let msq_path = args
        .next()
        .expect("usage: msq_dump <ini> <msq> [SYMBOL,...]");
    let symbols: std::collections::HashSet<String> = args
        .next()
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    let ini_text = read_text(&ini_path);
    let def = Arc::new(
        opentune_ini::parse_definition_with_symbols(&ini_text, &symbols).expect("parse ini"),
    );
    let mut tune = opentune_model::Tune::new(Arc::clone(&def));

    let xml = read_text(&msq_path);
    let report = match opentune_project::msq::load_msq_into(&mut tune, &xml) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("LOAD FAILED: {e}");
            std::process::exit(1);
        }
    };

    println!("applied: {}", report.applied);
    println!("skipped: {}", report.skipped.len());
    println!("failed : {}", report.failed.len());

    println!("\n--- all failed ---");
    for (name, reason) in report.failed.iter() {
        println!("  {name}: {reason}");
    }

    println!("\n--- first 20 skipped ---");
    for name in report.skipped.iter().take(20) {
        println!("  {name}");
    }

    println!("\n--- veTable sample ---");
    match tune.get("veTable") {
        Ok(v) => println!(
            "  {:?}",
            format!("{v:?}").chars().take(300).collect::<String>()
        ),
        Err(e) => println!("  ERROR: {e:?}"),
    }
    for name in ["launchRpm", "rpmHardLimit", "engineType", "injector_flow"] {
        match tune.get(name) {
            Ok(v) => println!("  {name} = {v:?}"),
            Err(e) => println!("  {name} ERROR: {e:?}"),
        }
    }
}
