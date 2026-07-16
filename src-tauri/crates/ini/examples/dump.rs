// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostic dump: parse an INI file and print what degraded.
//! Usage: cargo run -p opentune-ini --example dump -- <path-to-ini> [SYMBOL,SYMBOL,...]
//!
//! The optional second argument seeds the preprocessor's active `#if`
//! symbols — pass the project's `project.properties` → `ecuSettings` list
//! (e.g. `SPEED_DENSITY,FAHRENHEIT`) to reproduce what the app resolves.

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: dump <ini-path> [SYMBOL,SYMBOL,...]");
    let symbols: std::collections::HashSet<String> = std::env::args()
        .nth(2)
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let bytes = std::fs::read(&path).expect("read ini");
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect());

    let def = match opentune_ini::parse_definition_with_symbols(&text, &symbols) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("PARSE FAILED: {e}");
            std::process::exit(1);
        }
    };

    println!("signature      : {:?}", def.comms.signature);
    println!("pages          : {}", def.pages.len());
    println!("constants      : {}", def.constants.len());
    println!("pc_variables   : {}", def.pc_variables.len());
    println!("menus          : {}", def.menus.len());
    println!("dialogs        : {}", def.dialogs.len());
    println!("tables         : {}", def.tables.len());
    println!("curves         : {}", def.curves.len());
    println!("output_channels: {}", def.output_channels.len());
    println!("gauges         : {}", def.gauges.len());

    println!("\n--- tables ---");
    for t in &def.tables {
        println!("  {} ({})", t.name, t.title);
    }

    println!("\n--- diagnostics ({}) ---", def.diagnostics.len());
    for d in &def.diagnostics {
        println!("  {d:?}");
    }
}
