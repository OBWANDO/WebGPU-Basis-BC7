// src/bindings.rs
use wasm_bindgen::prelude::*;
use js_sys::Uint8Array;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = BASIS)]
    pub type BasisFile;

    #[wasm_bindgen(js_namespace = BASIS, js_name = initTranscoders)]
    pub fn init_transcoders();

    #[wasm_bindgen(js_namespace = BASIS, js_name = initializeBasis)]
    pub fn initialize_basis();

    #[wasm_bindgen(constructor, js_namespace = BASIS)]
    pub fn new(data: &Uint8Array) -> BasisFile;

    #[wasm_bindgen(method, js_name = startTranscoding)]
    pub fn start_transcoding(this: &BasisFile) -> bool;

    #[wasm_bindgen(method, js_name = getImageWidth)]
    pub fn get_image_width(this: &BasisFile, image_index: u32, level_index: u32) -> u32;

    #[wasm_bindgen(method, js_name = getImageHeight)]
    pub fn get_image_height(this: &BasisFile, image_index: u32, level_index: u32) -> u32;

    #[wasm_bindgen(method, js_name = transcodeImage)]
    pub fn transcode_image(
        this: &BasisFile,
        dst: &mut [u8],
        image_index: u32,
        level_index: u32,
        format: u32,
        unused1: u32,
        unused2: u32,
    ) -> bool;
}
