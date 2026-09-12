# syntax=docker/dockerfile:1
# Static `avdumpr` image: a single binary on `scratch`.
#   docker build -t avdumpr .
#   docker run --rm -v "$PWD:/data" avdumpr --Cons=ED2K,CRC32 --PrintHashes /data/video.mkv

# Built natively on each target platform (buildx runs the arm64 stage under QEMU); rust on Alpine
# links a static musl binary by default.
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
RUN cargo fetch --locked
RUN cargo build --release --locked --offline && cp target/release/avdumpr /avdumpr

FROM scratch
COPY --from=build /avdumpr /avdumpr
COPY LICENSE /LICENSE
WORKDIR /data
ENTRYPOINT ["/avdumpr"]
CMD ["--Help"]
