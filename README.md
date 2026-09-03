# LazyBoy

Create a bot in the browser, give it a Team or Private computer, and let it drive a Linux desktop.

## Run (local)

Postgres is on `127.0.0.1:5434` so it does not collide with other stacks.

```bash
cp .env.example .env          # set XAI_API_KEY for chat
docker compose up -d postgres
./scripts/build-computer-image.sh
DATA_DIR=/root/LazyBoy/data cargo run -p lazyboy-supervisor
DATABASE_URL=postgres://lazyboy:lazyboy@127.0.0.1:5434/lazyboy \
SANDBOX_PROVIDER=docker \
SANDBOX_SUPERVISOR_URL=http://127.0.0.1:7091 \
DATA_DIR=/root/LazyBoy/data \
LAZYBOY_WEB_DIR=apps/web \
API_BIND=0.0.0.0:3101 \
cargo run -p lazyboy-api
```

Open `http://<host>:3101`. The computer is a real Debian container: fluxbox toolbar, Chromium with tabs and URL bar, and xterm. Do not replace that with a kiosk or HTML landing page. The display is proxied through the API so you do not open extra ports.

For frontend development, run `npm install && npm run dev` in `apps/web`, then open
`http://127.0.0.1:5173`. Vite proxies API and computer-screen traffic to the Rust API on port 3101.

Model providers: v1 talks to xAI (`XAI_API_KEY`). `openai` / `anthropic` / `openrouter` are reserved on the factory and return `unsupported_provider`.
