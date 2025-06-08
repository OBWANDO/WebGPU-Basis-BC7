setlocal

cargo build --target wasm32-unknown-unknown
@echo off
if %ERRORLEVEL% NEQ 0 (
    echo Build failed.
    exit /b %ERRORLEVEL%
)

echo ====== Running wasm-bindgen... =======

wasm-bindgen --keep-debug --target web --out-dir static ./target/wasm32-unknown-unknown/debug/basis_webgpu_adaptive.wasm

@echo off
if %ERRORLEVEL% NEQ 0 (
    echo wasm-bindgen failed.
    exit /b %ERRORLEVEL%
)

echo ====== Build complete. Ready for browser debugging ======

