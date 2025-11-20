#!/bin/sh

rm -rf target/
rm -rf pkg/

wasm-pack build --target web
python3 -m http.server 8000
