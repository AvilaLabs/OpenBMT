//! Test-fixture audit for exact local/device monitor state equality.
//!
//! This deliberately uses the public BIP-85 regtest fixture seed from
//! `shop_regtest.rs`; it must never be pointed at a real shop's state.
//!
//! Usage: `shop_monitor_audit <local-state-dir> <device-fixture-dir> <index>`

use bitcoin::bip32::Xpriv;
use bitcoin::hashes::{sha256, Hash};
use openbmt_lightning_node::bip85::node_seed;
use openbmt_lightning_node::seal::{open, SealKey};
use openbmt_lightning_node::store::{device_name, split_device_name};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

const XPRV: &str = "xprv9s21ZrQH143K2LBWUUQRFXhucrQqBpKdRRxNVq2zBqsx8HVqFk2uYo8kmbaLLHRdqtQpUm98uKfu3vca1LqdGhUtyoFnCNkfmXRyPXLjbKb";

fn collect_local(
    root: &Path,
    category: &str,
    out: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), Box<dyn Error>> {
    let base = root.join(category);
    if !base.exists() {
        return Ok(());
    }
    let mut stack = vec![base.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let ty = entry.file_type()?;
            if ty.is_dir() {
                stack.push(path);
            } else if ty.is_file() {
                let rel = path.strip_prefix(&base)?;
                let parts: Vec<String> = rel
                    .components()
                    .map(|c| match c {
                        Component::Normal(s) => s.to_string_lossy().into_owned(),
                        _ => String::new(),
                    })
                    .collect();
                if parts.is_empty() || parts.iter().any(String::is_empty) {
                    return Err(format!("unexpected local store path: {}", path.display()).into());
                }
                let (secondary, key) = if category == "monitors" {
                    if parts.len() != 1 {
                        return Err(format!("unexpected monitor path: {}", path.display()).into());
                    }
                    (String::new(), parts[0].clone())
                } else {
                    if parts.len() != 2 {
                        return Err(
                            format!("unexpected monitor update path: {}", path.display()).into(),
                        );
                    }
                    (parts[0].clone(), parts[1].clone())
                };
                let name = device_name(category, &secondary, &key);
                if out.insert(name.clone(), fs::read(path)?).is_some() {
                    return Err(format!("duplicate local object {name}").into());
                }
            }
        }
    }
    Ok(())
}

fn collect_device(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((primary, _, _)) = split_device_name(&name) else {
            continue;
        };
        if (primary == "monitors" || primary == "monitor_updates")
            && out.insert(name.clone(), fs::read(entry.path())?).is_some()
        {
            return Err(format!("duplicate device object {name}").into());
        }
    }
    Ok(out)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err(format!("usage: {} LOCAL_STATE DEVICE_DIR INDEX", args[0]).into());
    }
    let index: u32 = args[3].parse()?;
    let local_root = PathBuf::from(&args[1]);
    let device_root = PathBuf::from(&args[2]);
    let master = Xpriv::from_str(XPRV)?;
    let key = SealKey::from_seed(&node_seed(&master, index)?);

    let mut local = BTreeMap::new();
    collect_local(&local_root, "monitors", &mut local)?;
    collect_local(&local_root, "monitor_updates", &mut local)?;
    let sealed = collect_device(&device_root)?;
    if local.is_empty() || sealed.is_empty() {
        return Err("refusing empty monitor inventory".into());
    }
    if local.len() != sealed.len() {
        return Err(format!(
            "inventory count differs: local={} device={}",
            local.len(),
            sealed.len()
        )
        .into());
    }

    for (name, plain) in &local {
        let blob = sealed
            .get(name)
            .ok_or_else(|| format!("device is missing {name}"))?;
        let opened =
            open(&key, name, blob).map_err(|e| format!("cannot authenticate {name}: {e:?}"))?;
        if &opened != plain {
            return Err(format!("plaintext differs for {name}").into());
        }
        let digest = sha256::Hash::hash(plain);
        println!("MATCH {name} sha256={digest}");
    }
    println!("PASS exact monitor state match: {} objects", local.len());
    Ok(())
}
