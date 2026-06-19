# Run the .NET netplay server (with the embedded Axiom engine) on Linux, where
# HTTP/3 / QUIC (libmsquic) is available — so WebTransport actually works. Build
# from the REPO ROOT as context:
#
#   podman build -f examples/axiom-netplay-dotnet/Dockerfile -t axiom-netplay .
#   podman run --rm -p 8090:8090 -p 8091:8091/udp axiom-netplay
#
# Then open http://localhost:8090/?transport=webtransport on the host.

# --- Stage 1: build the engine FFI cdylib (libaxiom_netplay_ffi.so) for Linux ---
FROM docker.io/library/rust:1-bookworm AS rust
WORKDIR /src
COPY . .
RUN cargo build -p axiom-netplay-ffi --release

# --- Stage 2: publish the .NET server ---
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS dotnet
WORKDIR /src
COPY examples/axiom-netplay-dotnet ./examples/axiom-netplay-dotnet
RUN dotnet publish examples/axiom-netplay-dotnet -c Release -o /app

# --- Final: aspnet runtime + libmsquic (for HTTP/3) ---
FROM mcr.microsoft.com/dotnet/aspnet:10.0
RUN apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && curl -fsSL https://packages.microsoft.com/config/debian/12/packages-microsoft-prod.deb -o /tmp/ms.deb \
 && dpkg -i /tmp/ms.deb \
 && apt-get update \
 && apt-get install -y --no-install-recommends libmsquic \
 && rm -rf /var/lib/apt/lists/* /tmp/ms.deb

WORKDIR /app
COPY --from=dotnet /app ./
COPY --from=rust /src/target/release/libaxiom_netplay_ffi.so /app/lib/
COPY apps/axiom-netplay-browser/web /app/web

ENV AXIOM_FFI_LIB=/app/lib/libaxiom_netplay_ffi.so
ENV AXIOM_WEB_ROOT=/app/web
ENV AXIOM_TRANSPORT=webtransport

EXPOSE 8090
EXPOSE 8091/udp
ENTRYPOINT ["dotnet", "axiom-netplay-dotnet.dll"]
