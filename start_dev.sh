#!/bin/bash

export RUST_LOG=${RUST_LOG:-dllb_server=debug,dllb_query=debug,dllb_storage=info}
export DLLB_BIND=${DLLB_BIND:-127.0.0.1:3009}
export DLLB_PATH=${DLLB_PATH:-dllb_dev.redb}
export DLLB_NS=${DLLB_NS:-default}
export DLLB_DB=${DLLB_DB:-default}

cargo run -p dllb-server -- "$@"
