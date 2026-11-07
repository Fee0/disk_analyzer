# Runs dscan-core's tests for Windows under Wine (x86 emulated, slow):
#
#   docker build --platform linux/amd64 -t dscan-wine -f ci/windows-wine.Dockerfile ci
#   docker run --rm --platform linux/amd64 -v "$PWD":/work -w /work \
#     -e CARGO_HOME=/work/target/docker-cargo-wine -e CARGO_TARGET_DIR=/work/target/docker-wine \
#     dscan-wine sh -c 'wineboot -i; cargo test -p dscan-core --target x86_64-pc-windows-gnu'
#
# Wine lacks FileIdExtdDirectoryInfo, so the reader takes its fallback and the
# two hard-link tests fail here; every other test must pass. Real NTFS
# behaviour still needs a Windows machine.
FROM --platform=linux/amd64 rust:1-trixie
RUN dpkg --add-architecture i386 && apt-get update && apt-get install -y --no-install-recommends wine wine64 mingw-w64 && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-pc-windows-gnu && rustup component add clippy
ENV CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine WINEDEBUG=-all LANG=C.UTF-8
