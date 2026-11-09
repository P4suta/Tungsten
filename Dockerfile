FROM node:26.13.0-alpine@sha256:8996bb5821d62a8fd9bb27c1cb07462f3fdce6e47d640c6a881ead0b064ce216 AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --ignore-scripts
COPY ui/ ./
RUN npm run build

FROM rust:1.99.0-alpine@sha256:0cce0a5e0e8ba67b455257a3a02a1d99005f382748789d6464460028810f1627 AS build
WORKDIR /app
ARG TARGETARCH
ENV CARGO_BUILD_JOBS=2
RUN apk add --no-cache make=4.4.1-r4 perl=5.42.3-r0
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY xtask/ xtask/
COPY src/ src/
COPY ui/ ui/
COPY --from=ui /ui/dist/ ui/dist/
COPY --from=ui /ui/node_modules/pdfjs-dist/ ui/node_modules/pdfjs-dist/
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/app/target,id=tungsten-target-${TARGETARCH} cargo build --release --locked --bin tungsten && mkdir -p /data /artifact && cp target/release/tungsten /artifact/tungsten

FROM scratch AS binaries
COPY --from=build /artifact/tungsten /tungsten
COPY LICENSE-MIT LICENSE-APACHE deploy/SQLCipher-LICENSE.txt /licenses/

FROM build AS scale-build
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/app/target,id=tungsten-target-${TARGETARCH} cargo build --release --locked --package xtask && cp target/release/xtask /artifact/xtask

FROM ui AS scale
COPY --from=scale-build /artifact/tungsten /tungsten
COPY --from=scale-build /artifact/xtask /xtask
RUN mkdir -p /data && chown 65532:65532 /data
USER 65532:65532
VOLUME ["/data"]
ENTRYPOINT ["/bin/busybox", "timeout", "-s", "TERM", "-k", "10", "890", "/xtask", "scale-native"]

FROM scale AS browser-performance
USER 0:0
RUN apk add --no-cache chromium=156.0.8078.79-r0 && npm --prefix assurance ci --ignore-scripts
ENV CHROME_PATH=/usr/bin/chromium XDG_CONFIG_HOME=/tmp/chromium-config XDG_CACHE_HOME=/tmp/chromium-cache
USER 65532:65532
ENTRYPOINT ["/bin/busybox", "timeout", "-s", "TERM", "-k", "10", "590", "/xtask", "browser-performance-native"]

FROM scratch AS runtime
COPY --from=build /artifact/tungsten /tungsten
COPY LICENSE-MIT LICENSE-APACHE deploy/SQLCipher-LICENSE.txt /licenses/
COPY --from=build --chown=65532:65532 /data /data
USER 65532:65532
EXPOSE 8080
VOLUME ["/data"]
ENTRYPOINT ["/tungsten"]
CMD ["--data-dir", "/data", "--listen", "0.0.0.0:8080"]
