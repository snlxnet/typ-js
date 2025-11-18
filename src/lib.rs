use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn add_one(x: u32) -> u32 {
    x + 1
}
