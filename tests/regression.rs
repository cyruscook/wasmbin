use anyhow::{Context, Result};
use std::io::Read;
use wasmbin::Module;
use wasmbin::types::HeapType;
use wasmbin::visit::Visit;

fn read_module<R: Read>(r: R) -> Result<Module> {
    let mut m = Module::decode_from(r).context("Decode error")?;
    m.visit_mut(|_: &mut HeapType| {})
        .context("Error visiting HeapTypes")?;
    Ok(m)
}

#[test]
fn heap_type_index_s33() -> Result<()> {
    let mod_bytes = include_bytes!("regression/heap_type_index_s33.wasm");
    let mut second_round = Vec::new();
    read_module(&mod_bytes[..])?.encode_into(&mut second_round)?;
    read_module(&second_round[..])?;
    Ok(())
}
