# Iniquity in a container. The image holds the server and the stock
# screens, menus, themes and scripts at /srv/inq/iniquity; a board
# directory (config, data, files, own screens) is mounted at
# /srv/<board>, so a board whose config points at ../inq/iniquity/...
# works the same in the container as beside a checkout.
#
#     docker build -t iniquity .
#     docker run -v /path/to/board:/srv/board -w /srv/board \
#         -p 2222:2222 -p 23:2323 iniquity
#
# deploy/docker-compose.yml does this for one board.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --release -p iniquity \
    && strip target/release/iniquity

FROM debian:bookworm-slim
# CA certificates for HTTPS (the AI chat and web search clients).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 1000 bbs
COPY --from=build /src/target/release/iniquity /usr/local/bin/iniquity
# The stock board files, where a board's config expects them.
COPY text /srv/inq/iniquity/text
COPY themes /srv/inq/iniquity/themes
COPY menus /srv/inq/iniquity/menus
COPY scripts /srv/inq/iniquity/scripts
USER bbs
WORKDIR /srv/board
EXPOSE 2222 2323
CMD ["iniquity", "iniquity.toml"]
