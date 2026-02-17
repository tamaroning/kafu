use anyhow::Result;
use std::collections::HashMap;
use wasmparser::{Parser, Payload};

#[derive(Debug, Clone)]
pub struct KafuModuleMetadata {
    pub functions: HashMap<u32, KafuFunctionMetadata>,
}

#[derive(Debug, Clone)]
pub struct KafuFunctionMetadata {
    pub name: Option<String>,
    /// Node ID specified by KAFU_DEST.
    /// Note: this is different from the actual migration destination.
    pub dest: Option<String>,
}

/// Precondition: data is a binary of the WASM module.
pub(crate) fn go(data: &[u8]) -> Result<KafuModuleMetadata> {
    let mut function_name_to_index_map = HashMap::new();
    for payload in Parser::new(0).parse_all(data) {
        match payload.expect("parse error") {
            Payload::ExportSection(section) => {
                for entry in section.into_iter_with_offsets() {
                    let (_, export) = entry?;
                    let name = export.name.to_string();
                    let func_idx = export.index;
                    function_name_to_index_map.insert(name, func_idx);
                }
            }
            Payload::End(_) => {
                break;
            }
            _ => {}
        }
    }

    let mut functions = HashMap::new();
    // read the custom `name` section and find ".kafu_dest."
    for payload in Parser::new(0).parse_all(data) {
        match payload.expect("parse error") {
            Payload::CustomSection(section) => {
                let name = section.name().to_string();
                if name.starts_with(".kafu_dest.") {
                    // .kafu_dest.ident.dest
                    let parts = name.split(".").collect::<Vec<&str>>();
                    let ident = parts[2];
                    let dest = parts[3];
                    tracing::debug!("found DEST {}@{}", ident, dest);
                    let index = function_name_to_index_map
                        .get(ident)
                        .expect("function not found");
                    let meta = KafuFunctionMetadata {
                        name: Some(ident.to_string()),
                        dest: Some(dest.to_string()),
                    };
                    functions.insert(*index, meta);
                }
            }
            Payload::End(_) => {
                break;
            }
            _ => {}
        }
    }
    tracing::debug!("Found {} KAFU_DEST", functions.len());
    Ok(KafuModuleMetadata { functions })
}
