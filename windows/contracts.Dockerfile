FROM node:26.13.0-alpine@sha256:8996bb5821d62a8fd9bb27c1cb07462f3fdce6e47d640c6a881ead0b064ce216 AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --ignore-scripts
COPY ui/ ./
RUN npm run build

FROM rust:1.99.0-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS server
WORKDIR /source
ENV CARGO_BUILD_JOBS=2
COPY Cargo.toml Cargo.lock ./
COPY xtask/ xtask/
COPY linux/ linux/
COPY src/ src/
COPY ui/ ui/
COPY --from=ui /ui/dist/ ui/dist/
COPY --from=ui /ui/node_modules/pdfjs-dist/ ui/node_modules/pdfjs-dist/
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/source/target cargo build --locked --bin tungsten && cp target/debug/tungsten /tungsten

FROM mcr.microsoft.com/dotnet/sdk:10.0.404@sha256:fe0fd4e93bd2fbb47c883b7a6369e5c8d3db8bf01cdc5981f6ee35fbdf3ff4fb AS contracts
WORKDIR /source
COPY windows/Directory.Build.props windows/Directory.Build.targets windows/global.json windows/
COPY windows/Tungsten.Core/ windows/Tungsten.Core/
COPY windows/Tungsten.Tests/ windows/Tungsten.Tests/
RUN dotnet restore windows/Tungsten.Tests/Tungsten.Tests.csproj --locked-mode
COPY --from=server /tungsten /usr/local/bin/tungsten
ENV TUNGSTEN_SERVER_PATH=/usr/local/bin/tungsten
CMD ["dotnet", "test", "windows/Tungsten.Tests/Tungsten.Tests.csproj", "--no-restore", "--filter", "Category=Server", "--nologo"]
