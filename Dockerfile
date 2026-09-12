# syntax=docker/dockerfile:1
# Static `avdump3` image: a single binary on `scratch`.
#   docker build --ssh default -t avdump3 .          (SSH agent forwards the key for mediainfo-rust)
#   docker run --rm -v "$PWD:/data" avdump3 --Cons=ED2K,CRC32 --PrintHashes /data/video.mkv

FROM --platform=$BUILDPLATFORM rust:1-alpine AS build
ARG TARGETARCH
RUN apk add --no-cache musl-dev git openssh-client \
    && mkdir -p -m 0700 ~/.ssh && ssh-keyscan github.com >> ~/.ssh/known_hosts 2>/dev/null
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY src ./src
COPY tests ./tests
# The mediainfo-rust dependency is fetched from a private git repository over SSH.
RUN --mount=type=ssh cargo fetch --locked
RUN case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-musl ;; \
      arm64) target=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported TARGETARCH $TARGETARCH" && exit 1 ;; \
    esac \
    && rustup target add "$target" \
    && cargo build --release --locked --offline --target "$target" \
    && cp "target/$target/release/avdump3" /avdump3

FROM scratch
COPY --from=build /avdump3 /avdump3
COPY LICENSE /LICENSE
WORKDIR /data
ENTRYPOINT ["/avdump3"]
CMD ["--Help"]
