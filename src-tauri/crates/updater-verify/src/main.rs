use std::{env, error::Error, path::PathBuf};

use opentune_updater_verify::verify_files;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1).map(PathBuf::from);
    let public_key = args
        .next()
        .ok_or("usage: opentune-updater-verify <public-key> <artifact> <signature>")?;
    let artifact = args
        .next()
        .ok_or("usage: opentune-updater-verify <public-key> <artifact> <signature>")?;
    let signature = args
        .next()
        .ok_or("usage: opentune-updater-verify <public-key> <artifact> <signature>")?;
    if args.next().is_some() {
        return Err("usage: opentune-updater-verify <public-key> <artifact> <signature>".into());
    }

    verify_files(&public_key, &artifact, &signature)?;
    println!("Verified updater signature for {}", artifact.display());
    Ok(())
}
