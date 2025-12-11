default:
    @just --list

release-linux:
    cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.39

publish: release-linux
    scp target/x86_64-unknown-linux-gnu/release/pluribus us:~/pluribus/pluribus

run *ARGS:
    cargo run -- {{ARGS}}

check:
    cargo fmt
    cargo clippy -- -D warnings
