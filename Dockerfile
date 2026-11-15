FROM node:26.13.1-alpine@sha256:6bd9ab43c0ec0190ca7b2d193906cb8b7edf6e7f4afa43ddeccd6452710bffb5 AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --ignore-scripts
COPY ui/ ./
RUN npm run build

FROM rust:1.99.1-alpine@sha256:44819b27be4c97b0e4d7203bd2e02c4820f97700211b6a37fb4c246a2de59383 AS build
WORKDIR /app
ARG TARGETARCH
ENV CARGO_BUILD_JOBS=2
COPY Cargo.toml Cargo.lock ./
COPY xtask/ xtask/
COPY linux/ linux/
COPY src/ src/
COPY ui/ ui/
COPY --from=ui /ui/dist/ ui/dist/
COPY --from=ui /ui/node_modules/pdfjs-dist/ ui/node_modules/pdfjs-dist/
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/app/target,id=tungsten-target-${TARGETARCH} cargo build --release --locked --bin tungsten && mkdir -p /data /artifact && cp target/release/tungsten /artifact/tungsten

FROM scratch AS binaries
COPY --from=build /artifact/tungsten /tungsten
COPY LICENSE-MIT LICENSE-APACHE /licenses/

FROM build AS scale-build
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/app/target,id=tungsten-target-${TARGETARCH} cargo build --release --locked --package xtask && cp target/release/xtask /artifact/xtask

FROM ui AS scale
ENV SQLITE_TMPDIR=/data
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

FROM scale AS memory-profile
ENTRYPOINT ["/bin/busybox", "timeout", "-s", "TERM", "-k", "5", "180", "/xtask", "memory-profile-native"]

FROM scale AS storage-profile
ENTRYPOINT ["/bin/busybox", "timeout", "-s", "TERM", "-k", "10", "890", "/xtask", "storage-profile-native", "--request", "/request.json"]

FROM scratch AS runtime
ENV SQLITE_TMPDIR=/data
COPY --from=build /artifact/tungsten /tungsten
COPY LICENSE-MIT LICENSE-APACHE /licenses/
COPY --from=build --chown=65532:65532 /data /data
USER 65532:65532
EXPOSE 8080
VOLUME ["/data"]
ENTRYPOINT ["/tungsten"]
CMD ["--data-dir", "/data", "--listen", "0.0.0.0:8080"]
