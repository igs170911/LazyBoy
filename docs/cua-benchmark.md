# Cua adapter timings (2026-09-07)

Measured on Apple Silicon, `lazyboy/computer:local`, Cua Driver 0.23.2, inside `make cua-smoke` (`--repeat 10`). These are LazyBoy `controld` HTTP calls, not raw `cua-driver` CLI.

| Call | n | median ms | max ms |
| --- | ---: | ---: | ---: |
| `GET /controller/health` | 1 | 11 | 11 |
| `POST /observe` | 30 | 101 | 125 |
| `POST /act` | 40 | 403 | 744 |
| `POST /browser` snapshot | 10 | 48 | 51 |
| `POST /browser` click | 20 | 332 | 337 |
| `POST /browser` type | 10 | 391 | 399 |
| `POST /browser` navigate | 10 | 927 | 941 |

`/act` includes native click, batched setvalue+click, focus, and one Cua `drag`. Navigate includes the 800 ms settle in the adapter.

There is no legacy control-plane comparison in this run. Do not treat these numbers as a ship gate against CDP/AT-SPI.
