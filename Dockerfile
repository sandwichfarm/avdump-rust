# syntax=docker/dockerfile:1
# Static `avdump3` image: a single binary on `scratch`.
#   docker build --ssh default -t avdump3 .          (SSH agent forwards the key for mediainfo-rust)
#   docker run --rm -v "$PWD:/data" avdump3 --Cons=ED2K,CRC32 --PrintHashes /data/video.mkv

# Built natively on each target platform (buildx runs the arm64 stage under QEMU); rust on Alpine
# links a static musl binary by default.
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev git openssh-client \
    && mkdir -p -m 0700 ~/.ssh && ssh-keyscan github.com >> ~/.ssh/known_hosts 2>/dev/null
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY src ./src
COPY tests ./tests
# The mediainfo-rust dependency is fetched from a private git repository over SSH.
RUN --mount=type=ssh cargo fetch --locked
RUN cargo build --release --locked --offline && cp target/release/avdump3 /avdump3

FROM scratch
COPY --from=build /avdump3 /avdump3
COPY LICENSE /LICENSE
WORKDIR /data
ENTRYPOINT ["/avdump3"]
CMD ["--Help"]
