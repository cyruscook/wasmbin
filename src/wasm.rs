use crate::Module;
use wasm_bindgen::prelude::*;

fn js_error(err: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&err.to_string())
}

/// Parse a WebAssembly binary from a JavaScript `Uint8Array`.
#[wasm_bindgen(js_name = fromBytes)]
#[expect(clippy::needless_pass_by_value, clippy::boxed_local)]
pub fn from_bytes(bytes: Box<[u8]>) -> Result<JsValue, JsValue> {
    let module = Module::decode_from(&*bytes).map_err(js_error)?;
    serde_wasm_bindgen::to_value(&module).map_err(js_error)
}

/// Encode the module into a JavaScript `Uint8Array`.
#[wasm_bindgen(js_name = toBytes)]
pub fn to_bytes(module: JsValue) -> Result<Box<[u8]>, JsValue> {
    serde_wasm_bindgen::from_value::<Module>(module)
        .map_err(js_error)?
        .encode_into(Vec::new())
        .map(Vec::into_boxed_slice)
        .map_err(js_error)
}
