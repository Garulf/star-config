//! `p4k_dump <Data.p4k> [entry]`: with no entry, print what the archive's defaults hold; with an
//! entry, print it as text (CryXmlB entries come out as XML).

use sc_core::cryxml;
use sc_core::defaults;
use sc_core::p4k::Archive;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [archive] => counts(archive),
        [archive, entry] => dump(archive, entry),
        _ => Err("usage: p4k_dump <Data.p4k> [entry]".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn open(path: &str) -> Result<Archive, String> {
    Archive::open(Path::new(path)).map_err(|err| err.to_string())
}

fn counts(path: &str) -> Result<(), String> {
    let archive = open(path)?;
    let data = defaults::from_archive(&archive, "p4k_dump", "english")
        .map_err(|reason| reason.reason())?;
    let counts = data.counts();
    println!("maps:          {}", counts.maps);
    println!("actions:       {}", counts.actions);
    println!("settings:      {}", counts.settings);
    println!("option trees:  {}", counts.option_trees);
    println!("option groups: {}", counts.groups);
    println!(
        "action labels: {} of {} resolve",
        counts.labels_found, counts.label_keys
    );
    Ok(())
}

fn dump(path: &str, entry: &str) -> Result<(), String> {
    let bytes = open(path)?.read(entry).map_err(|err| err.to_string())?;
    let text = cryxml::decode_entry(bytes)
        .or_else(String::from_utf8)
        .map_err(|_| format!("{entry} is neither XML nor text"))?;
    println!("{text}");
    Ok(())
}
