# syntax=docker/dockerfile:1
# Real bot desktop. Every boot must look like a Linux workstation:
# XFCE panel + window manager, Chromium, zsh terminal. Never a kiosk/HTML shell.
# Traditional Chinese fonts/locale so CJK text does not mojibake.
# Build from the repository root:
#   docker build -f image/computer/Dockerfile -t lazyboy/computer:local .

# Pinned; must satisfy the workspace rust-version in Cargo.toml.
FROM rust:1.98.1-bookworm AS controld
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
COPY migrations migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=lazyboy-computer-release,sharing=locked \
    cargo build --locked --release -p lazyboy-controld && cp /src/target/release/lazyboy-controld /lazyboy-controld

# Cua 0.23.2 embeds Inter for session badges, which has no Chinese glyphs.
# Build the same pinned release with our existing OFL Huninn UI font embedded.
# The cosmetic patch reads the bot's selected color; input/permissions are unchanged.
FROM rust:1.97.1-bookworm AS cua_driver
RUN apt-get update && apt-get install -y --no-install-recommends \
    libx11-dev libxi-dev libxtst-dev libxrandr-dev libxfixes-dev libxkbcommon-dev \
    libwayland-dev pkg-config patch && rm -rf /var/lib/apt/lists/*
WORKDIR /src
RUN curl -fsSL https://codeload.github.com/trycua/cua/tar.gz/refs/tags/cua-driver-rs-v0.23.2 -o /tmp/cua.tar.gz \
    && echo '151c72982c9f06bf168bd00f9611cef6760f2fc2459916dc216810ac24200f3d  /tmp/cua.tar.gz' | sha256sum -c \
    && tar -xzf /tmp/cua.tar.gz --strip-components=1 \
    && rm /tmp/cua.tar.gz
RUN curl -fsSL https://github.com/justfont/open-huninn-font/releases/download/v2.1/jf-openhuninn-2.1.ttf -o /tmp/huninn.ttf \
    && echo '9d5bf4932d31fe94c18cd8cfddc98bc1b14ce10f4e354c682179db290a99c825  /tmp/huninn.ttf' | sha256sum -c \
    && cp /tmp/huninn.ttf libs/cua-driver/rust/crates/cursor-overlay/assets/Inter.ttf
WORKDIR /src/libs/cua-driver/rust
COPY image/computer/cua-color.rs crates/cursor-overlay/src/lazyboy_color.rs
COPY image/computer/cua-color.patch /tmp/cua-color.patch
RUN patch --batch --fuzz=0 -p1 < /tmp/cua-color.patch
COPY image/computer/cua-browser-cursor.rs crates/platform-linux/src/lazyboy_browser_cursor.rs
COPY image/computer/cua-browser-cursor.patch /tmp/cua-browser-cursor.patch
RUN patch --batch --fuzz=0 -p1 < /tmp/cua-browser-cursor.patch
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/libs/cua-driver/rust/target,id=lazyboy-cua-cjk,sharing=locked \
    cargo build --locked --release -p cua-driver --features portal-input \
    && cp target/release/cua-driver /cua-driver

FROM debian:bookworm-slim
ARG TARGETARCH

# Keep Chromium, XFCE, CJK, AT-SPI, git. Skip Debian novnc (pulls nodejs) and
# fonts-noto-core (Latin is DejaVu/Liberation/huninn).
# dpkg never unpacks docs/man/info/foreign locales; zh_TW catalogs are kept so
# XFCE menus are in Traditional Chinese. apt lists/archives live in cache
# mounts, so nothing has to be removed from the layer afterwards.
# Only Papirus (base) + Papirus-Dark icons are used; the Dark variant symlinks
# into the base set, so the other variants are dead weight (~60 MB).
RUN --mount=type=cache,id=lazyboy-apt-cache-$TARGETARCH,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,id=lazyboy-apt-lists-$TARGETARCH,target=/var/lib/apt,sharing=locked \
    rm -f /etc/apt/apt.conf.d/docker-clean \
  && printf '%s\n' \
      'path-exclude=/usr/share/doc/*' \
      'path-include=/usr/share/doc/*/copyright' \
      'path-exclude=/usr/share/man/*' \
      'path-exclude=/usr/share/info/*' \
      'path-exclude=/usr/share/gtk-doc/*' \
      'path-exclude=/usr/share/backgrounds/*' \
      'path-exclude=/usr/share/locale/*' \
      'path-include=/usr/share/locale/locale.alias' \
      'path-include=/usr/share/locale/zh_TW/*' \
      'path-include=/usr/share/locale/zh/*' \
      > /etc/dpkg/dpkg.cfg.d/zz-lazyboy-slim \
  && apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    chromium \
    curl \
    dbus-x11 \
    fonts-dejavu-core \
    fonts-liberation \
    fonts-noto-cjk \
    fonts-noto-color-emoji \
    git \
    htop \
    locales \
    procps \
    python3 \
    sudo \
    gosu \
    util-linux \
    websockify \
    x11-utils \
    x11vnc \
    xdg-utils \
    xfce4-panel \
    xfce4-settings \
    xfce4-terminal \
    xfconf \
    xfdesktop4 \
    xfwm4 \
    xvfb \
    thunar \
    adwaita-icon-theme \
    gnome-themes-extra \
    arc-theme \
    papirus-icon-theme \
    librsvg2-common \
    at-spi2-core \
    libatk-adaptor \
    python3-gi \
    gir1.2-atspi-2.0 \
    gir1.2-gtk-3.0 \
    libxi6 \
    libxkbcommon0 \
    zsh \
  && echo "zh_TW.UTF-8 UTF-8" >> /etc/locale.gen \
  && echo "en_US.UTF-8 UTF-8" >> /etc/locale.gen \
  && locale-gen \
  && mkdir -p /usr/share/novnc \
  && rm -rf /usr/share/i18n /usr/share/icons/ePapirus /usr/share/icons/ePapirus-Dark /usr/share/icons/Papirus-Light \
  && test -e /usr/share/icons/Papirus-Dark/48x48/apps/utilities-terminal.svg \
  && test -e /usr/share/icons/Papirus-Dark/48x48/apps/web-browser.svg \
  && test -d /usr/share/themes/Arc-Dark/xfwm4 \
  && find /usr -name __pycache__ -type d -prune -exec rm -rf '{}' + \
  && find /usr -name '*.py[co]' -delete

# MesloLGS NF (Powerlevel10k glyphs, CJK via fontconfig). Pinned to a
# powerlevel10k-media commit; jf open 粉圓 (system UI) is the same verified
# file the Cua stage embeds, copied below.
ARG MESLO_BASE=https://raw.githubusercontent.com/romkatv/powerlevel10k-media/145eb9fbc2f42ee408dacd9b22d8e6e0e553f83d
RUN mkdir -p /usr/share/fonts/truetype/huninn /usr/share/fonts/truetype/meslo \
  && cd /usr/share/fonts/truetype/meslo \
  && curl -fsSL -o "MesloLGS NF Regular.ttf" "$MESLO_BASE/MesloLGS%20NF%20Regular.ttf" \
  && curl -fsSL -o "MesloLGS NF Bold.ttf" "$MESLO_BASE/MesloLGS%20NF%20Bold.ttf" \
  && curl -fsSL -o "MesloLGS NF Italic.ttf" "$MESLO_BASE/MesloLGS%20NF%20Italic.ttf" \
  && curl -fsSL -o "MesloLGS NF Bold Italic.ttf" "$MESLO_BASE/MesloLGS%20NF%20Bold%20Italic.ttf" \
  && printf '%s\n' \
      'd97946186e97f8d7c0139e8983abf40a1d2d086924f2c5dbf1c29bd8f2c6e57d  MesloLGS NF Regular.ttf' \
      'b6c0199cf7c7483c8343ea020658925e6de0aeb318b89908152fcb4d19226003  MesloLGS NF Bold.ttf' \
      '6f357bcbe2597704e157a915625928bca38364a89c22a4ac36e7a116dcd392ef  MesloLGS NF Italic.ttf' \
      '56b4131adecec052c4b324efb818dd326d586dbc316fc68f98f1cae2eb8d1220  MesloLGS NF Bold Italic.ttf' \
      | sha256sum -c
COPY --from=cua_driver --chmod=644 /tmp/huninn.ttf /usr/share/fonts/truetype/huninn/jf-openhuninn-2.1.ttf
RUN fc-cache -f

# zsh prompt + plugins from pinned release tarballs (reproducible, no build-time git).
RUN set -e; mkdir -p /tmp/zsh /usr/share/zsh/plugins; \
  fetch() { \
    curl -fsSL -o "/tmp/zsh/$1.tar.gz" "$2" \
    && echo "$3  /tmp/zsh/$1.tar.gz" | sha256sum -c \
    && mkdir -p "$4" && tar -xzf "/tmp/zsh/$1.tar.gz" --strip-components=1 -C "$4"; \
  }; \
  fetch powerlevel10k https://codeload.github.com/romkatv/powerlevel10k/tar.gz/refs/tags/v1.20.0 \
    d8187d44b697b3a37a8c4896678b4380e717cbf2850179529358348780a2d3d7 /usr/share/zsh-theme-powerlevel10k; \
  fetch zsh-autosuggestions https://codeload.github.com/zsh-users/zsh-autosuggestions/tar.gz/refs/tags/v0.7.1 \
    0df7affff21cd87ed298e6a3970ed08a1dd66a6efa676454ee5b091ad503badf /usr/share/zsh/plugins/zsh-autosuggestions; \
  fetch zsh-syntax-highlighting https://codeload.github.com/zsh-users/zsh-syntax-highlighting/tar.gz/refs/tags/0.8.0 \
    5981c19ebaab027e356fe1ee5284f7a021b89d4405cc53dc84b476c3aee9cc32 /usr/share/zsh/plugins/zsh-syntax-highlighting; \
  fetch zsh-z https://codeload.github.com/agkozak/zsh-z/tar.gz/102fb78036ed76feedf623907483691777a1d510 \
    90edc058f50447d27915accd18e022b4059fbaf1d1b34408034fb8f14eba5b9b /usr/share/zsh/plugins/zsh-z; \
  rm -rf /tmp/zsh; \
  test -f /usr/share/zsh-theme-powerlevel10k/powerlevel10k.zsh-theme; \
  test -f /usr/share/zsh/plugins/zsh-z/zsh-z.plugin.zsh

RUN useradd --create-home --uid 1000 --shell /bin/zsh lazyboy \
    && mkdir -p /home/lazyboy /tmp/lazyboy /usr/share/lazyboy/skel /usr/share/lazyboy/xfce-skel /etc/gtk-3.0 /etc/fonts/conf.d \
    && chown -R 1000:1000 /home/lazyboy /tmp/lazyboy

# Install outside the persisted /home/lazyboy bind-mount.
COPY --from=cua_driver --chmod=755 /cua-driver /usr/local/lib/cua-driver/cua-driver
COPY --from=cua_driver /src/LICENSE.md /usr/share/licenses/cua-driver/LICENSE.md
COPY image/computer/licenses/huninn-OFL.txt /usr/share/licenses/cua-driver/huninn-OFL.txt
RUN ln -sf /usr/local/lib/cua-driver/cua-driver /usr/local/bin/cua-driver \
    && cua-driver --version

COPY --from=controld --chmod=755 /lazyboy-controld /usr/local/bin/lazyboy-controld
COPY --chmod=755 image/computer/rotate-logs.py /usr/local/bin/lazyboy-rotate-logs
COPY --chmod=755 image/computer/lazyboy-screen /usr/local/bin/lazyboy-screen
COPY --chmod=755 image/computer/lazyboy-browser /usr/local/bin/lazyboy-browser
COPY --chmod=755 image/computer/lazyboy-terminal /usr/local/bin/lazyboy-terminal
COPY --chmod=755 image/computer/lazyboy-terminal-reset /usr/local/bin/lazyboy-terminal-reset
COPY --chmod=755 image/computer/lazyboy-clipboard /usr/local/bin/lazyboy-clipboard
COPY --chmod=644 apps/web/vnc.html /usr/share/novnc/vnc_lite.html
COPY --chmod=644 apps/web/vnc.html /usr/share/novnc/index.html
COPY --chmod=644 image/computer/fonts.conf /etc/fonts/conf.d/99-lazyboy-cjk.conf
COPY --chmod=644 image/computer/gtk3-settings.ini /etc/gtk-3.0/settings.ini
COPY --chmod=644 image/computer/dotfiles/zshrc /usr/share/lazyboy/skel/zshrc
COPY --chmod=644 image/computer/dotfiles/p10k.zsh /usr/share/lazyboy/skel/p10k.zsh
COPY --chmod=644 image/computer/dotfiles/terminalrc /usr/share/lazyboy/skel/terminalrc
COPY --chmod=644 image/computer/xfce/xfce4-panel.xml /usr/share/lazyboy/xfce-skel/xfce4/xfconf/xfce-perchannel-xml/xfce4-panel.xml
COPY --chmod=644 image/computer/xfce/xfwm4.xml /usr/share/lazyboy/xfce-skel/xfce4/xfconf/xfce-perchannel-xml/xfwm4.xml
COPY --chmod=644 image/computer/xfce/xfce4-desktop.xml /usr/share/lazyboy/xfce-skel/xfce4/xfconf/xfce-perchannel-xml/xfce4-desktop.xml
COPY --chmod=644 image/computer/xfce/thunar.xml /usr/share/lazyboy/xfce-skel/xfce4/xfconf/xfce-perchannel-xml/thunar.xml
COPY --chmod=644 image/computer/xfce/wallpaper.svg /usr/share/lazyboy/wallpaper.svg
COPY --chmod=644 image/computer/xfce/terminal.desktop /usr/share/applications/lazyboy-terminal.desktop
COPY --chmod=644 image/computer/xfce/browser.desktop /usr/share/applications/lazyboy-browser.desktop
# Traditional Chinese catalogs were retained during package installation.
# Hide stock xterm / duplicate terminal entries; the panel launches
# lazyboy-terminal (zsh -l).
RUN for f in xterm uxterm debian-xterm xfce4-terminal; do \
       if [ -f "/usr/share/applications/${f}.desktop" ]; then \
         printf '\nNoDisplay=true\nHidden=true\n' >> "/usr/share/applications/${f}.desktop"; \
       fi; \
     done \
  && chmod -R a+rX /usr/share/lazyboy /usr/share/applications /usr/share/novnc \
  && chmod -R a+rX /usr/share/locale/zh_TW /usr/share/locale/zh 2>/dev/null || true \
  && test -f /usr/share/locale/zh_TW/LC_MESSAGES/xfce4-terminal.mo
# After xfce4-helpers reinstall, override preferred apps so the menu
# 終端機模擬程式 / 網路瀏覽器 launch zsh + lazyboy-browser, not xterm.
COPY --chmod=644 image/computer/xfce/helpers.rc /etc/xdg/xfce4/helpers.rc
COPY --chmod=644 image/computer/xfce/helper-terminal.desktop /usr/share/xfce4/helpers/lazyboy-terminal.desktop
COPY --chmod=644 image/computer/xfce/helper-browser.desktop /usr/share/xfce4/helpers/lazyboy-browser.desktop
COPY --chmod=755 image/computer/chromium /usr/local/bin/chromium
RUN sed -i 's|^Exec=/usr/bin/chromium|Exec=/usr/local/bin/lazyboy-browser|' /usr/share/applications/chromium.desktop || true

COPY --chmod=755 image/computer/start.sh /usr/local/bin/lazyboy-computer
COPY --chmod=755 image/computer/entrypoint.sh /usr/local/bin/lazyboy-entrypoint
COPY --chmod=644 image/computer/cua-smoke.html /usr/share/lazyboy/cua-smoke.html
COPY --chmod=755 image/computer/cua-smoke-gtk.py /usr/local/bin/lazyboy-cua-smoke-gtk
COPY --chmod=755 scripts/cua-smoke-inner.py /usr/local/bin/lazyboy-cua-smoke
COPY --chmod=755 scripts/cua-adapter-test.py /usr/local/bin/lazyboy-cua-adapter-test
COPY --chmod=755 scripts/cua-terminal-test.py /usr/local/bin/lazyboy-cua-terminal-test
COPY --chmod=755 scripts/cua-clipboard-test.py /usr/local/bin/lazyboy-cua-clipboard-test
COPY --chmod=755 scripts/cua-session-test.py /usr/local/bin/lazyboy-cua-session-test
COPY --chmod=755 scripts/cua-cursor-test.py /usr/local/bin/lazyboy-cua-cursor-test
COPY --chmod=755 scripts/cua-cursor-color-test.py /usr/local/bin/lazyboy-cua-cursor-color-test
COPY --chmod=755 scripts/cua-observe-http-test.py /usr/local/bin/lazyboy-cua-observe-http-test
COPY --chmod=755 scripts/cua-isolation-test.py /usr/local/bin/lazyboy-cua-isolation-test
COPY --chmod=755 scripts/cua-smoke-test.sh /usr/local/bin/lazyboy-cua-smoke-host

USER root
ENV HOME=/home/lazyboy DISPLAY=:1 SHELL=/bin/zsh TERM=xterm-256color \
    LANG=zh_TW.UTF-8 LC_ALL=zh_TW.UTF-8 LANGUAGE=zh_TW:zh:en \
    GTK_MODULES=atk-bridge GTK_A11Y=atspi GNOME_ACCESSIBILITY=1 NO_AT_BRIDGE=0
WORKDIR /home/lazyboy
EXPOSE 6080 6081 6082 6083 6084 6085 6086 6087
CMD ["/usr/local/bin/lazyboy-entrypoint"]
