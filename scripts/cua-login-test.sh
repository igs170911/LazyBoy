#!/usr/bin/env bash
# End-to-end saved-login field selection and typing in a disposable Cua desktop.
# The local certificate is trusted only inside this throwaway container.
set -euo pipefail
image="${COMPUTER_IMAGE:-lazyboy/computer:local}"
name=$(docker run -d --shm-size=512m -e LAZYBOY_CONTROL_TOKEN=login-fixture-only "$image")
trap 'docker rm -f "$name" >/dev/null 2>&1 || true' EXIT
for _ in $(seq 1 120); do
  if docker exec "$name" test -f /tmp/lazyboy/ready; then break; fi
  sleep 1
done
docker exec "$name" test -f /tmp/lazyboy/ready
docker exec -u 0 "$name" sh -c 'apt-get update -qq && apt-get install -y -qq libnss3-tools' >/dev/null 2>&1
docker exec "$name" openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout /tmp/login-key.pem -out /tmp/login-cert.pem -days 2 \
  -subj /CN=localhost -addext subjectAltName=DNS:localhost \
  -addext basicConstraints=critical,CA:TRUE >/dev/null 2>&1
docker exec -u 1000:1000 "$name" sh -c '
  mkdir -p "$HOME/.pki/nssdb"
  certutil -N --empty-password -d sql:"$HOME/.pki/nssdb"
  certutil -A -d sql:"$HOME/.pki/nssdb" -n lazyboy-local-fixture -t "C,," -i /tmp/login-cert.pem
'
root="$(cd "$(dirname "$0")/.." && pwd)"
docker cp "$root/scripts/cua-login-fixture.py" "$name":/tmp/login-fixture.py
docker exec -d "$name" python3 /tmp/login-fixture.py
CUA_LOGIN_TEST_CONTAINER="$name" cargo test --manifest-path "$root/Cargo.toml" \
  -p lazyboy-api saved_login_fills_real_cua_fields_without_submitting -- --ignored
