<div align="center">

<img src="./apps/web/public/lazyboy-icon.png" width="96" height="96" alt="LazyBoy" />

**English** · [繁體中文](./README.zh-TW.md)

# LazyBoy

**Give AI a computer so it can do the work.**

A self-hosted AI agent workspace. Assign tasks in text or voice, watch the desktop live, and take over whenever you need to.

[Quick start](#quick-start) · [Features](#features) · [Architecture](./docs/architecture.md) · [Operations](./docs/operations.md) · [Development](./docs/development.md)

</div>

![LazyBoy group chat routing one message to a single agent, with that agent's live desktop on the right](./docs/readme-hero.png)

LazyBoy gives each agent its own Linux desktop in Docker — browser, terminal, and files. You can run several agents, put them in a group, turn a demonstration into a skill, and schedule it to run again.

This is an early `v0.1.0-alpha` release with desktop and phone browser UIs. You bring your own model API key.

## Features

- **A lasting workspace**: each agent has its own chats, run history, and optional long-term memory.
- **A real computer**: open pages, use the terminal, organize files, drive the GUI — and watch it live.
- **Attachments in chat**: send files or images along with the message; the agent can open them on its own desktop, and inbox copies expire on their own.
- **Take over any time**: sign in, pass a check, or nudge things by hand on the same desktop, then hand it back.
- **Saved logins**: keep site credentials in the agent's encrypted vault, so it can fill them in at a login wall without the password ever passing through the model.
- **Several agents and groups**: shared Team computers or private dedicated desktops; `@name` decides who answers, so a message wakes the one agent it is for instead of all of them.
- **Teach by demo, then schedule**: turn a walkthrough into a skill; use cron for repeat work.
- **Your models and tools**: xAI, OpenCode Go, OpenAI-compatible endpoints, MCP, and file skills.
- **Voice calls**: after you enable a voice provider, you can talk to the agent on a call.
- **Phone-friendly**: collapse the chat sidebar; the remote desktop has keyboard, trackpad, right-click, drag, and scroll.
- **English and Traditional Chinese**: switch the UI language in the app.

## Quick start

You need Docker and Compose, Git, Make, Python 3, and an API key for a supported model. On macOS use Docker Desktop or OrbStack; on Linux use Docker Engine.

From the repo root:

```bash
make env
```

Edit the generated `.env` and set one of `XAI_API_KEY`, `OPENCODE_GO_API_KEY`, or `OPENAI_API_KEY`. The init tool creates the login and service secrets; running it again keeps existing values.

```bash
make up
make health
```

Open **[http://127.0.0.1:3101](http://127.0.0.1:3101)**, sign in with `LAZYBOY_APP_TOKEN` from `.env`, create an agent, and pick a model.

The first run builds the API and Linux desktop images from source and takes a while. The build toolchain lives in the containers, so a full Docker deploy does not need Rust or Node.js on the host.

Try a concrete task:

> Open the site I name, summarize the page, and save the notes as a Markdown file in the workspace.

Status and logs:

```bash
make ps
make logs
make down  # stop services, keep PostgreSQL data
```

For resource limits, environment variables, HTTPS, and in-container sudo, see [Operations](./docs/operations.md).

## On a phone

Desktop and phone share the same web UI. The API listens on `127.0.0.1` by default, so set `LAZYBOY_BIND_IP=0.0.0.0` (or one network card's address) in `.env` and recreate the api container before a phone on your network can reach it; off-loopback the login token has to be at least 32 characters. Then open that address in the phone browser — `127.0.0.1` there is the phone itself and will not reach another machine.

Tap outside the chat sidebar to collapse it. On the remote desktop you can switch between tap-to-click and trackpad, and use the toolbar for keyboard, right-click, or drag. Put the service behind HTTPS before you expose it; see [Operations](./docs/operations.md#安全模型).

## Stack

- **Frontend**: React 19, TypeScript, Vite, noVNC
- **Backend**: Rust 2024, Axum, Tokio
- **Data**: PostgreSQL, pgvector, SQLx
- **Desktop**: Docker, Debian, XFCE, Chromium, Xvfb
- **Computer control**: Cua Driver over X11, AT-SPI, and Chromium
- **Extensions**: MCP, file skills, demonstration playbooks

Flow diagrams, handoff, component roles, and the computer lifecycle live in **[Architecture](./docs/architecture.md)**. The older **[interactive diagram](./docs/workflow.html)** is still there — download it and open it in a browser.

## Local development

Besides Docker, you need a Rust toolchain that supports the 2024 edition, plus Node.js / npm.

```bash
make dev
make dev-supervisor  # terminal 1
make dev-api         # terminal 2
```

Frontend hot reload in another terminal:

```bash
cd apps/web
npm install
npm run dev
```

Open [http://127.0.0.1:5173](http://127.0.0.1:5173). Tests and the tree layout are in the [development guide](./docs/development.md).

## Docs

The guides below are currently in Traditional Chinese.

| Doc                                                      | Contents                                             |
| -------------------------------------------------------- | ---------------------------------------------------- |
| [Architecture](./docs/architecture.md)                   | Task flow, system architecture, computer lifecycle   |
| [Interactive diagram](./docs/workflow.html)              | Zoomable, searchable HTML chart; download and open   |
| [Operations](./docs/operations.md)                       | Resources, env vars, security, site checks, sudo     |
| [Agent experience](./docs/agent-experience.md)           | Turn limits, persistent terminal, live chat          |
| [hermes-agent review](./docs/hermes-agent-cua-review.md) | Cua harness smoothness: comparison with hermes-agent |
| [Development](./docs/development.md)                     | Local dev, checks and tests, directory layout        |
| [Env example](./.env.example)                            | Environment variables and defaults                   |

## Data

Chats, memory, browser profiles, and encrypted credentials stay on your host. When you use an external model, the prompts, tool results, and screenshots the task needs may still be sent to that provider.

## Acknowledgements

LazyBoy is mostly other people's software, carefully assembled. The projects we lean on hardest:

**Special thanks**

- **[Cua](https://github.com/trycua/cua)** — the Linux desktop driver behind every click, keystroke, and screenshot. We rebuild `cua-driver-rs` v0.23.2 from source with two small patches kept in this repo: one lets the agent cursor wear the bot's own colour, the other swaps the embedded Latin-only badge font for [jf open Huninn](https://github.com/justfont/open-huninn-font) so Chinese renders. Neither patch touches input handling or permissions.
- **[hermes-agent](https://github.com/NousResearch/hermes-agent)** — not a dependency, but the reference we kept returning to while tuning how smooth an agent's desktop should feel. The comparison is written up in the [hermes-agent review](./docs/hermes-agent-cua-review.md).

**Agents and retrieval** — [rig](https://github.com/0xPlaygrounds/rig) · [rmcp](https://github.com/modelcontextprotocol/rust-sdk) · [fastembed-rs](https://github.com/Anush008/fastembed-rs) and [ONNX Runtime](https://github.com/microsoft/onnxruntime), running [paraphrase-multilingual-MiniLM-L12-v2](https://huggingface.co/sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2)

**Rust** — Tokio · Axum · tower-http · SQLx · reqwest · rustls · Bollard · tracing · aes-gcm and hmac from [RustCrypto](https://github.com/RustCrypto) · cap-std · cron · chrono · uuid · thiserror · dotenvy

**Web** — React · Vite · TypeScript · [noVNC](https://github.com/novnc/noVNC) · react-markdown with [remark-gfm](https://github.com/remarkjs/remark-gfm) and [remark-breaks](https://github.com/remarkjs/remark-breaks) · [Blobatar](https://github.com/Alain00/blobatar) avatars · [react-useanimations](https://github.com/useAnimations/react-useanimations) icons

**Data** — [PostgreSQL](https://www.postgresql.org) · [pgvector](https://github.com/pgvector/pgvector)

**The desktop inside each container** — [Docker](https://www.docker.com) · [Debian](https://www.debian.org) · [XFCE](https://www.xfce.org) · [Chromium](https://www.chromium.org) · [Xvfb](https://www.x.org) · Thunar · [x11vnc](https://github.com/LibVNC/x11vnc) · [websockify](https://github.com/novnc/websockify) · [AT-SPI2](https://gitlab.gnome.org/GNOME/at-spi2-core) · [gosu](https://github.com/tianon/gosu) · [LXCFS](https://github.com/lxc/lxcfs) · git · zsh with [Powerlevel10k](https://github.com/romkatv/powerlevel10k) · htop

**Type and theme** — [jf open Huninn](https://github.com/justfont/open-huninn-font) (SIL OFL) in the UI and on the agent badge · [Noto CJK](https://github.com/notofonts/noto-cjk), DejaVu, and Liberation as fallbacks · [MesloLGS NF](https://github.com/romkatv/powerlevel10k-media) in the terminal · [Arc Dark](https://github.com/horst3180/arc-theme) and [Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme) for the look of the room

Every version above is pinned in `Cargo.lock`, `apps/web/package.json`, and `image/computer/Dockerfile`. LazyBoy is Apache-2.0; each project keeps its own license. Thanks also to the maintainers whose names never make it into a README, and to everyone who files a good bug report.

---

<div align="center">

<img src="./apps/web/public/lazyboy-icon.png" width="72" height="72" alt="LazyBoy" />

**LazyBoy is free and open source.** If it hands you back an afternoon, a coffee is the nicest way to say so.

<a href="https://www.buymeacoffee.com/daniel.wang.1993"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me a Coffee" height="48" /></a>

<img src="https://img.shields.io/badge/license-Apache--2.0-2f9e8f?style=for-the-badge&labelColor=1b1b22" alt="Apache License 2.0" />
<img src="https://img.shields.io/badge/release-v0.1.0--alpha-2f9e8f?style=for-the-badge&labelColor=1b1b22" alt="Release v0.1.0-alpha" />
<img src="https://img.shields.io/badge/Rust%20%2B%20React-6f9c96?style=for-the-badge&labelColor=1b1b22" alt="Rust and React" />

**Daniel Wang** <img src="https://flagcdn.com/w20/tw.png" width="20" alt="Taiwan" />

[igs170911@gmail.com](mailto:igs170911@gmail.com) · [Apache License 2.0](./LICENSE)

</div>
