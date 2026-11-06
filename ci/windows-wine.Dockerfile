FROM --platform=linux/amd64 rust:1-trixie
RUN dpkg --add-architecture i386 && apt-get update && apt-get install -y --no-install-recommends wine wine64 mingw-w64 && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-pc-windows-gnu && rustup component add clippy
ENV CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine WINEDEBUG=-all LANG=C.UTF-8
