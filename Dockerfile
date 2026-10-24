# Discophile web player: the WebAssembly build served by an unprivileged nginx on port 8080.
# The wasm is architecture-independent, so it is built once on the build platform for every image architecture.

FROM --platform=$BUILDPLATFORM rust:1-bookworm AS build
ARG TRUNK_VERSION=0.22.0
RUN rustup target add wasm32-unknown-unknown \
 && curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-$(uname -m)-unknown-linux-musl.tar.gz" \
    | tar -xz -C /usr/local/bin trunk
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cd crates/app && trunk build --release && rm -f /src/dist/.trunk-lock \
 && find /src/dist -type f \( -name '*.wasm' -o -name '*.js' -o -name '*.html' \) -exec gzip -9 -k {} \;

FROM nginxinc/nginx-unprivileged:stable-alpine
ARG VERSION=dev
LABEL org.opencontainers.image.title="Discophile web player" \
      org.opencontainers.image.description="Your music server, played like a record collection. A web player for Navidrome, Subsonic, Jellyfin and Audiobookshelf." \
      org.opencontainers.image.url="https://discophile.app" \
      org.opencontainers.image.source="https://github.com/debba/discophile" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="${VERSION}"
USER root
RUN rm -f /etc/nginx/conf.d/default.conf \
 && install -d -o nginx -g nginx /etc/nginx/discophile
COPY deploy/docker/nginx.conf /etc/nginx/conf.d/discophile.conf
COPY deploy/docker/headers.conf /etc/nginx/discophile-headers.conf
COPY --chmod=755 deploy/docker/40-discophile-proxies.sh /docker-entrypoint.d/
COPY --from=build /src/dist /usr/share/nginx/html
USER nginx
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s CMD wget -q -O /dev/null http://127.0.0.1:8080/healthz || exit 1
