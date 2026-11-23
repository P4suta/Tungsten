FROM node:26.13.1-alpine@sha256:6bd9ab43c0ec0190ca7b2d193906cb8b7edf6e7f4afa43ddeccd6452710bffb5 AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --ignore-scripts
COPY ui/ ./
RUN npm run build

FROM rust:1.100.0-bookworm@sha256:ade39a710ef85f3506cffcfec3e8a24ba090badef1dff2fb798f71d878833fea AS verification-tools
ENV CARGO_BUILD_JOBS=2
RUN cargo install --locked --version 0.68.0 kani-verifier && cargo kani setup
ENTRYPOINT ["timeout", "--signal=TERM", "--kill-after=10s", "890s"]

FROM verification-tools AS release-tools
RUN cargo install --locked --version 0.13.2 cargo-fuzz && cargo install --locked --version 27.1.0 cargo-mutants
RUN rustup toolchain install nightly-2026-10-01 --profile minimal

FROM verification-tools AS proofs
WORKDIR /project
COPY src/rules.rs src/rules.rs
COPY src/rules/ src/rules/
COPY linux/model/src/ linux/model/src/
COPY proofs/Cargo.toml proofs/Cargo.lock proofs/
COPY proofs/src/ proofs/src/
COPY target/browser-proof.rs target/browser-proof.rs
ENV CARGO_NET_OFFLINE=true
WORKDIR /reports
CMD ["cargo", "kani", "--manifest-path", "/project/proofs/Cargo.toml", "-Z", "unstable-options", "--export-json", "/reports/kani-results.json", "--harness-timeout", "300s", "--output-format", "terse", "--log-file", "/reports/kani.log"]

FROM release-tools AS release-check
ENV CARGO_NET_OFFLINE=false
ENV CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
WORKDIR /project
COPY Cargo.toml Cargo.lock ./
COPY .cargo/config.toml .cargo/config.toml
COPY xtask/ xtask/
COPY linux/ linux/
COPY src/ src/
COPY tests/ tests/
COPY fuzz/ fuzz/
COPY proofs/Cargo.toml proofs/Cargo.lock proofs/
COPY proofs/src/ proofs/src/
COPY target/browser-proof.rs target/browser-proof.rs
COPY ui/ ui/
COPY --from=ui /ui/dist/ ui/dist/
COPY --from=ui /ui/node_modules/pdfjs-dist/ ui/node_modules/pdfjs-dist/
RUN --mount=type=cache,target=/project/target,id=tungsten-native-task-amd64,sharing=locked cargo build --locked --package xtask && install --mode=755 target/debug/xtask /usr/local/bin/tungsten-xtask
CMD ["tungsten-xtask", "release-native"]
