FROM node:26.13.0-alpine@sha256:8996bb5821d62a8fd9bb27c1cb07462f3fdce6e47d640c6a881ead0b064ce216 AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --ignore-scripts
COPY ui/ ./
RUN npm run build

FROM rust:1.99.0-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS proofs
ENV CARGO_BUILD_JOBS=2
RUN cargo install --locked --version 0.68.0 kani-verifier && cargo kani setup
WORKDIR /project
COPY src/rules.rs src/rules.rs
COPY src/rules/ src/rules/
COPY linux/model/src/ linux/model/src/
COPY proofs/Cargo.toml proofs/Cargo.lock proofs/
COPY proofs/src/ proofs/src/
COPY target/browser-proof.rs target/browser-proof.rs
ENV CARGO_NET_OFFLINE=true
WORKDIR /reports
ENTRYPOINT ["timeout", "--signal=TERM", "--kill-after=10s", "890s"]
CMD ["cargo", "kani", "--manifest-path", "/project/proofs/Cargo.toml", "-Z", "unstable-options", "--export-json", "/reports/kani-results.json", "--harness-timeout", "300s", "--output-format", "terse", "--log-file", "/reports/kani.log"]

FROM proofs AS release-check
ENV CARGO_NET_OFFLINE=false
RUN cargo install --locked --version 0.13.2 cargo-fuzz && cargo install --locked --version 27.1.0 cargo-mutants
RUN rustup toolchain install nightly-2026-10-01 --profile minimal
WORKDIR /project
COPY Cargo.toml Cargo.lock ./
COPY xtask/ xtask/
COPY linux/ linux/
COPY src/ src/
COPY tests/ tests/
COPY fuzz/ fuzz/
COPY ui/ ui/
COPY --from=ui /ui/dist/ ui/dist/
COPY --from=ui /ui/node_modules/pdfjs-dist/ ui/node_modules/pdfjs-dist/
CMD ["cargo", "xtask", "release-native"]
