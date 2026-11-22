FROM node:26.13.1-alpine@sha256:6bd9ab43c0ec0190ca7b2d193906cb8b7edf6e7f4afa43ddeccd6452710bffb5 AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --ignore-scripts
COPY ui/ ./
RUN npm run build

FROM rust:1.100.0-bookworm@sha256:ade39a710ef85f3506cffcfec3e8a24ba090badef1dff2fb798f71d878833fea AS server
WORKDIR /source
ENV CARGO_BUILD_JOBS=2
COPY Cargo.toml Cargo.lock ./
COPY xtask/ xtask/
COPY linux/ linux/
COPY src/ src/
COPY assets/ assets/
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
