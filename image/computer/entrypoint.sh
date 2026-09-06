#!/usr/bin/env bash
set -euo pipefail

if [[ "${LAZYBOY_COMPUTER_SUDO:-false}" =~ ^(1|true|yes)$ ]]; then
  printf 'lazyboy ALL=(ALL) NOPASSWD:ALL\n' > /etc/sudoers.d/lazyboy
  chmod 0440 /etc/sudoers.d/lazyboy
else
  rm -f /etc/sudoers.d/lazyboy
fi

exec gosu lazyboy /usr/local/bin/lazyboy-computer
