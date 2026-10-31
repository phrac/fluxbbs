# flux/bbs in a container. The image holds the server and the stock
# screens, menus, themes and scripts at /srv/inq/iniquity; a board
# directory (config, data, files, own screens) is mounted at
# /srv/<board>, so a board whose config points at ../inq/iniquity/...
# works the same in the container as beside a checkout.
#
#     docker build -t fluxbbs .
#     docker run -v /path/to/board:/srv/board -w /srv/board \
#         -p 2222:2222 -p 23:2323 fluxbbs
#
# That's the core board. FEATURES picks optional components
# (docs/BUILDING.md), e.g. --build-arg FEATURES=full for all of them,
# or FEATURES="doors games effects". deploy/docker-compose.yml does
# this for one board, with everything on.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
ARG FEATURES=""
RUN cargo build --release -p fluxbbs ${FEATURES:+--features "$FEATURES"} \
    && strip target/release/fluxbbs

FROM debian:bookworm-slim
# CA certificates for HTTPS (the AI chat, BBSLink and the textfiles
# importer); Poppler's tools to show PDFs (pdftotext, pdftoppm,
# pdfinfo), with the `pdf` component.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates poppler-utils \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 1000 bbs
COPY --from=build /src/target/release/fluxbbs /usr/local/bin/fluxbbs
# The stock board files, where a board's config expects them (the
# path keeps the old name, which existing configs use).
COPY text /srv/inq/iniquity/text
COPY themes /srv/inq/iniquity/themes
COPY menus /srv/inq/iniquity/menus
COPY scripts /srv/inq/iniquity/scripts
# Iniquity's own screens and menus, for a board that wants them back.
COPY classic /srv/inq/iniquity/classic
USER bbs
WORKDIR /srv/board
EXPOSE 2222 2323
# fluxbbs.toml, or iniquity.toml on a board from before the rename.
CMD ["fluxbbs"]
