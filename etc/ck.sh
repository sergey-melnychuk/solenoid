#!/bin/sh

BLOCK=$1
SKIP=$2

cargo run --release --example check -- $BLOCK $SKIP --compact
