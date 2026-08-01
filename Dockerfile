# One command to a runnable artifact:
#     docker build -t fuzzy-rs .
#     docker run --rm -i fuzzy-rs --help
#     printf 'DMETAPHONE\t0\tmayer\n' | docker run --rm -i fuzzy-rs
#
# The runtime stage is also the rule-05 evidence: it contains the port and
# nothing else. No interpreter is installed, so the claim "this artifact does
# not link the source language" is checkable rather than merely asserted:
#
#     docker run --rm --entrypoint sh fuzzy-rs -c 'command -v python python3 || echo no python'
#     docker run --rm --entrypoint sh fuzzy-rs -c 'ldd /usr/local/bin/fuzzy'

FROM rust:1-slim AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked

FROM debian:stable-slim
LABEL org.opencontainers.image.title="fuzzy-rs"
LABEL org.opencontainers.image.description="Rust port of yougov/fuzzy — Soundex, NYSIIS, Double Metaphone"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.source="https://github.com/yougov/fuzzy"
COPY --from=build /src/target/release/fuzzy /usr/local/bin/fuzzy
ENTRYPOINT ["/usr/local/bin/fuzzy"]
