# Building flux/bbs

flux/bbs is a core board plus optional components. A plain build is the
core: messages, files, transfers, chat, QWK and the rest of what a BBS
is. The components are Cargo features you turn on when you build.
disflux, the flagship board, runs with all of them.

    cargo build --release                         # the core board
    cargo build --release --features full         # everything
    cargo build --release --features "doors games effects"

## The components

| Feature | What it adds | Needs |
|---|---|---|
| `doors` | DOS doors in the built-in DOS machine (`D0`-`D4`, `D7`, `DZ`), external doors, and BBSLink (`DB`) | the `fluxbbs-dos` crate; HTTPS for BBSLink |
| `effects` | Menus and screens that animate while they wait (`[[effect]]`, `NAME.FX`), and the picture switching off at logoff (`HI`, `HQ`) | |
| `screensavers` | The screen savers: digital rain, starfield and fire, and their font picker in `*C` | `effects` |
| `casino` | The casino (`AC`) and the slot machine (`AL`, and the free pull at logon) | |
| `carrierlost` | Carrier Lost (`AP`) | |
| `tarot` | The tarot and fortunes (`AT`) | `ai`, which it brings: the AI reads the cards |
| `games` | All three games: `casino`, `carrierlost`, `tarot` | |
| `ai` | AI chat (`AI`), the AI on the message boards, and the board's memory of callers | HTTPS; an OpenAI-style model server (`[ai]`) |
| `ftn` | FTN echomail: packets, tossing, binkp, `fluxbbs ftn` | |
| `meshcore` | The MeshCore radio bridge | a MeshCore companion radio |
| `mrc` | MRC (Multi Relay Chat), live chat with callers on other BBSes (`XR`); off until `[mrc] enabled` | an outbound connection to an MRC server (TLS, port 5001) |
| `pdf` | Showing PDFs in the file browser and art gallery | Poppler (`poppler-utils`) on the host |
| `python` | Python scripts on menus (`-X`) and in strings (`%!`), with the `fluxbbs` module | Python 3 on the host (`python3`; the Docker image has it) |
| `textfiles` | The textfiles.com importer (`fluxbbs textfiles`) | HTTPS |
| `weather` | The weather (`WX`, or `WX <place>` for a fixed one): the sky in animated pixels, the week, the next 24 hours; the AI reads it as a forecaster when `ai` is built and on. `[weather]`: `units`, `place`, `ai_forecast`, `ai_effort` (the forecaster's `reasoning_effort`, `none` unless set) | HTTPS to open-meteo.com (free, no key) |
| `email` | Callers' email addresses, confirmed by a mailed code (`UV`, the `email` logon step, `*C` `@`); off until `[email] enabled` | an SMTP server |
| `full` | All of the above | |

Features combine freely: name as many as you like, space- or
comma-separated. `screensavers` brings `effects` with it, and `tarot`
(so `games`) brings `ai`.

## What a board without a component does

- **Its commands answer as if switched off.** They say `FeatureOff`, as
  a part turned off in `*C` Features does, wherever they're typed or
  scripted.
- **Menus leave it out.** Lightbars, the generic lists, `/?` help and
  the logon sequence skip its entries, so a stock menu works on any
  build.
- **Its settings still load.** The TOML sections for it (`[casino]`,
  `[ai]`, `[ftn]`...) are read and kept, and `*C` still edits them, so
  the same config works whichever way the board is built.
- **Its command-line tools say so.** `fluxbbs ftn` and `fluxbbs
  textfiles` stop with a note to build with the feature.
- **Where it touches the core, the core goes on without it.** Without
  `effects` nothing animates; without `screensavers` Backspace on a
  menu does nothing; without `pdf` a PDF is a file to download;
  without `meshcore` posts simply aren't bridged; without `python` a `%!`
  line in a screen shows nothing; without `email` the `email` logon
  step drops out.

## In a container

The Dockerfile builds the core unless told otherwise:

    docker build --build-arg FEATURES=full -t fluxbbs .
    docker build --build-arg FEATURES="doors games" -t fluxbbs .

`deploy/docker-compose.yml` builds with everything (`FEATURES=full`), as
disflux runs. Set `FEATURES` in the environment to change it:

    FEATURES="doors effects screensavers" \
        docker compose -f deploy/docker-compose.yml up -d --build

`tools/setup_host.sh` builds with everything too; `FEATURES=` (empty)
builds the core.

## How big

Stripped release binaries, x86-64 Linux, 26 Oct 2026:

| Build | Size |
|---|---|
| core | 20.8 MB |
| full | 27.9 MB |

## Working on flux/bbs

Test and lint both ends: the full build, and the core alone.

    cargo test --features full
    cargo test
    cargo clippy --all-targets --features full
    cargo clippy --all-targets

A new optional part gets a feature in `crates/fluxbbs/Cargo.toml`
(and in `full`), its modules behind `#[cfg(feature = "...")]` in
`main.rs`, and its commands in `features::feature_of` and
`features::built`, so a build without it refuses them the usual way.
Where core code calls into it, give the core a stand-in (as the
effects' `Ambient` and the MeshCore outbox have) rather than scattering
`cfg`s through the callers. To check every component on its own:

    for f in doors effects screensavers casino carrierlost tarot ai ftn meshcore mrc pdf python textfiles; do
        cargo clippy -p fluxbbs --all-targets --no-default-features --features $f
    done
