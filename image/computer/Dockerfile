# syntax=docker/dockerfile:1
# Real bot desktop. Every boot must look like a Linux workstation:
# XFCE panel + window manager, Chromium, zsh terminal. Never a kiosk/HTML shell.
# Traditional Chinese fonts/locale so CJK text does not mojibake.
# Build from the repository root:
#   docker build -f image/computer/Dockerfile -t lazyboy/computer:local .

FROM rust:1-bookworm AS controld
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY migrations migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=lazyboy-computer-release,sharing=locked \
    cargo build --locked --release -p lazyboy-controld && cp /src/target/release/lazyboy-controld /lazyboy-controld

FROM debian:bookworm-slim

# Keep Chromium, XFCE, CJK, AT-SPI, git. Skip Debian novnc (pulls nodejs),
# fonts-noto-core (Latin is DejaVu/Liberation/huninn), and unused xterm.
RUN printf '%s\n' \
      'path-include=/usr/share/locale/zh_TW/*' \
      'path-include=/usr/share/locale/zh/*' \
      > /etc/dpkg/dpkg.cfg.d/zz-lazyboy-locale \
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
    imagemagick \
    locales \
    procps \
    python3 \
    python3-websocket \
    sudo \
    gosu \
    util-linux \
    websockify \
    wmctrl \
    x11-apps \
    x11-utils \
    x11vnc \
    xclip \
    xdg-utils \
    xfce4-panel \
    xfce4-settings \
    xfce4-terminal \
    xfconf \
    xfdesktop4 \
    xdotool \
    xfwm4 \
    xvfb \
    thunar \
    adwaita-icon-theme \
    gnome-themes-extra \
    librsvg2-common \
    at-spi2-core \
    libatk-adaptor \
    python3-gi \
    gir1.2-atspi-2.0 \
    zsh \
  && echo "zh_TW.UTF-8 UTF-8" >> /etc/locale.gen \
  && echo "en_US.UTF-8 UTF-8" >> /etc/locale.gen \
  && locale-gen \
  && mkdir -p /usr/share/novnc \
  && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/* /usr/share/doc/* /usr/share/man/*

# jf open 粉圓 (system UI) + MesloLGS NF (Powerlevel10k glyphs, CJK via fontconfig).
RUN mkdir -p /usr/share/fonts/truetype/huninn /usr/share/fonts/truetype/meslo \
  && curl -fsSL -o /usr/share/fonts/truetype/huninn/jf-openhuninn-2.1.ttf \
      https://github.com/justfont/open-huninn-font/releases/download/v2.1/jf-openhuninn-2.1.ttf \
  && curl -fsSL -o "/usr/share/fonts/truetype/meslo/MesloLGS NF Regular.ttf" \
      "https://github.com/romkatv/powerlevel10k-media/raw/master/MesloLGS%20NF%20Regular.ttf" \
  && curl -fsSL -o "/usr/share/fonts/truetype/meslo/MesloLGS NF Bold.ttf" \
      "https://github.com/romkatv/powerlevel10k-media/raw/master/MesloLGS%20NF%20Bold.ttf" \
  && curl -fsSL -o "/usr/share/fonts/truetype/meslo/MesloLGS NF Italic.ttf" \
      "https://github.com/romkatv/powerlevel10k-media/raw/master/MesloLGS%20NF%20Italic.ttf" \
  && curl -fsSL -o "/usr/share/fonts/truetype/meslo/MesloLGS NF Bold Italic.ttf" \
      "https://github.com/romkatv/powerlevel10k-media/raw/master/MesloLGS%20NF%20Bold%20Italic.ttf" \
  && fc-cache -f

RUN git clone --depth=1 https://github.com/romkatv/powerlevel10k.git /usr/share/zsh-theme-powerlevel10k \
  && git clone --depth=1 https://github.com/zsh-users/zsh-autosuggestions.git /usr/share/zsh/plugins/zsh-autosuggestions \
  && git clone --depth=1 https://github.com/zsh-users/zsh-syntax-highlighting.git /usr/share/zsh/plugins/zsh-syntax-highlighting \
  && git clone --depth=1 https://github.com/agkozak/zsh-z.git /usr/share/zsh/plugins/zsh-z \
  && rm -rf /usr/share/zsh-theme-powerlevel10k/.git \
            /usr/share/zsh/plugins/zsh-autosuggestions/.git \
            /usr/share/zsh/plugins/zsh-syntax-highlighting/.git \
            /usr/share/zsh/plugins/zsh-z/.git

RUN useradd --create-home --uid 1000 --shell /bin/zsh lazyboy \
    && mkdir -p /home/lazyboy /tmp/lazyboy /usr/share/lazyboy/skel /usr/share/lazyboy/xfce-skel /etc/gtk-3.0 /etc/fonts/conf.d \
    && chown -R 1000:1000 /home/lazyboy /tmp/lazyboy

COPY --from=controld --chmod=755 /lazyboy-controld /usr/local/bin/lazyboy-controld
COPY --chmod=755 image/computer/rotate-logs.py /usr/local/bin/lazyboy-rotate-logs
COPY --chmod=755 image/computer/lazyboy-screen /usr/local/bin/lazyboy-screen
COPY --chmod=755 image/computer/lazyboy-browser /usr/local/bin/lazyboy-browser
COPY --chmod=755 image/computer/lazyboy-terminal /usr/local/bin/lazyboy-terminal
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

USER root
ENV HOME=/home/lazyboy DISPLAY=:1 SHELL=/bin/zsh TERM=xterm-256color \
    LANG=zh_TW.UTF-8 LC_ALL=zh_TW.UTF-8 LANGUAGE=zh_TW:zh:en \
    GTK_MODULES=atk-bridge GTK_A11Y=atspi GNOME_ACCESSIBILITY=1 NO_AT_BRIDGE=0
WORKDIR /home/lazyboy
EXPOSE 6080 6081 6082 6083 6084 6085 6086 6087
CMD ["/usr/local/bin/lazyboy-entrypoint"]
