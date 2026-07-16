// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostic dump: parse an INI file and print what degraded.
//! Usage: cargo run -p opentune-ini --example dump -- <path-to-ini>

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump <ini-path>");
    let bytes = std::fs::read(&path).expect("read ini");
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect());

    let def = match opentune_ini::parse_definition(&text) {
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
