# LazyBoy

Create a bot in the browser, give it a Team or Private computer, and let it drive a Linux desktop.

## Run (local)

Postgres is on `127.0.0.1:5434` so it does not collide with other stacks.

```bash
cp .env.example .env
# Set XAI_API_KEY, then generate two different secrets:
# openssl rand -hex 32
# openssl rand -hex 32
# Put them in LAZYBOY_APP_TOKEN and SANDBOX_SUPERVISOR_TOKEN.
docker compose up -d postgres
./scripts/build-computer-image.sh
SANDBOX_SUPERVISOR_TOKEN=<same-strong-supervisor-token> \
DATA_DIR=/root/LazyBoy/data cargo run -p lazyboy-supervisor
DATABASE_URL=postgres://lazyboy:lazyboy@127.0.0.1:5434/lazyboy \
SANDBOX_PROVIDER=docker \
SANDBOX_SUPERVISOR_URL=http://127.0.0.1:7091 \
SANDBOX_SUPERVISOR_TOKEN=<same-strong-supervisor-token> \
LAZYBOY_APP_TOKEN=<strong-app-token> \
DATA_DIR=/root/LazyBoy/data \
LAZYBOY_WEB_DIR=apps/web \
API_BIND=0.0.0.0:3101 \
cargo run -p lazyboy-api
```

Open `http://<host>:3101` and sign in with `LAZYBOY_APP_TOKEN`. The computer is a real Debian container: fluxbox toolbar, Chromium with tabs and URL bar, and xterm. The display is proxied through the authenticated API; the supervisor is internal-only in Docker Compose and must not be published to the LAN.

For LAN use, put LazyBoy behind HTTPS whenever possible. A shared token sent over plain HTTP can be observed by other devices on an untrusted network. Set `LAZYBOY_SECURE_COOKIE=true` when HTTPS terminates at LazyBoy or a trusted reverse proxy. The API refuses a non-loopback bind unless `LAZYBOY_APP_TOKEN` is at least 32 characters; the supervisor likewise rejects missing, short, or default tokens.

For frontend development, run `npm install && npm run dev` in `apps/web`, then open
`http://127.0.0.1:5173`. Vite proxies API and computer-screen traffic to the Rust API on port 3101.

The standalone supervisor listens on `127.0.0.1:7091` by default. Docker Compose reaches it internally as `supervisor:7091`; there is intentionally no host port `7092`.

Model providers: v1 talks to xAI (`XAI_API_KEY`). `openai` / `anthropic` / `openrouter` are reserved on the factory and return `unsupported_provider`.
