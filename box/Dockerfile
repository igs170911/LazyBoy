# LazyBoy's computer: a real Linux desktop the human (noVNC) and the agent
# (xdotool + AT-SPI + CDP) share. Looks like lazyBoy's computer image:
# Arc-Dark + Papirus + jf open 粉圓, Traditional Chinese, Taipei time.
# One TigerVNC X server replaces Xvfb + x11vnc; no XFCE session manager.
FROM debian:bookworm-slim
ENV DEBIAN_FRONTEND=noninteractive
# dpkg never unpacks docs/man/foreign locales; zh_TW catalogs are kept so the
# XFCE menus are in Traditional Chinese.
RUN printf '%s\n' \
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
        tigervnc-standalone-server tigervnc-tools x11-utils x11-xserver-utils \
        novnc websockify xclip imagemagick xdotool \
        dbus-x11 xfconf xfce4-panel xfwm4 xfdesktop4 xfce4-settings xfce4-terminal \
        arc-theme papirus-icon-theme adwaita-icon-theme gnome-themes-extra librsvg2-common \
        at-spi2-core libatk-adaptor python3-gi gir1.2-atspi-2.0 gir1.2-gtk-3.0 \
        dconf-gsettings-backend gsettings-desktop-schemas libglib2.0-bin \
        fonts-dejavu-core fonts-liberation fonts-noto-cjk fonts-noto-color-emoji \
        locales tzdata \
        python3 curl ca-certificates git bash tini procps \
        chromium \
    && echo "zh_TW.UTF-8 UTF-8" >> /etc/locale.gen \
    && echo "en_US.UTF-8 UTF-8" >> /etc/locale.gen \
    && locale-gen \
    && ln -sf /usr/share/zoneinfo/Asia/Taipei /etc/localtime \
    && echo "Asia/Taipei" > /etc/timezone \
    && rm -rf /usr/share/icons/ePapirus /usr/share/icons/ePapirus-Dark /usr/share/icons/Papirus-Light \
    && test -e /usr/share/icons/Papirus-Dark/48x48/apps/web-browser.svg \
    && test -d /usr/share/themes/Arc-Dark/xfwm4 \
    && rm -rf /var/lib/apt/lists/*
# jf open 粉圓 (OFL) as the desktop UI font, pinned like lazyBoy.
RUN mkdir -p /usr/share/fonts/truetype/huninn \
    && curl -fsSL https://github.com/justfont/open-huninn-font/releases/download/v2.1/jf-openhuninn-2.1.ttf \
         -o /usr/share/fonts/truetype/huninn/jf-openhuninn-2.1.ttf \
    && echo '9d5bf4932d31fe94c18cd8cfddc98bc1b14ce10f4e354c682179db290a99c825  /usr/share/fonts/truetype/huninn/jf-openhuninn-2.1.ttf' | sha256sum -c \
    && fc-cache -f
COPY --from=node:22-bookworm-slim /usr/local/ /usr/local/
COPY playwright/ /opt/lazyboy/playwright/
RUN cd /opt/lazyboy/playwright && PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 npm install --omit=dev
# Grok Build CLI (`grok`): bots hand coding work to it through the grok-build
# skill. Login (`grok login`) is per bot and persists in /home/box/.grok.
ARG GROK_CLI_VERSION=1.0.50
RUN npm install -g "@xai-official/grok@${GROK_CLI_VERSION}" && npm cache clean --force
RUN mkdir -p /workspace /home/box /usr/share/lazyboy/xfce-skel /etc/xdg/xfce4 /usr/share/xfce4/helpers /etc/gtk-3.0 /etc/fonts/conf.d /usr/share/licenses/huninn \
    && chmod 1777 /workspace
COPY --chmod=755 start-desktop.sh keep-desktop-awake desktop-probe.py stop-browser.py stop-browser.mjs restore-browser.mjs box-chrome box-terminal box-a11y /usr/local/bin/
COPY --chmod=644 welcome.html /usr/share/lazyboy/welcome.html
COPY --chmod=644 xfce/wallpaper.svg /usr/share/lazyboy/wallpaper.svg
COPY --chmod=644 xfce/xfce4-panel.xml xfce/xfwm4.xml xfce/xfce4-desktop.xml /usr/share/lazyboy/xfce-skel/
COPY --chmod=644 xfce/helpers.rc /etc/xdg/xfce4/helpers.rc
COPY --chmod=644 xfce/helper-browser.desktop /usr/share/xfce4/helpers/lazyboy-browser.desktop
COPY --chmod=644 xfce/helper-terminal.desktop /usr/share/xfce4/helpers/lazyboy-terminal.desktop
COPY --chmod=644 xfce/browser.desktop /usr/share/applications/lazyboy-browser.desktop
COPY --chmod=644 xfce/terminal.desktop /usr/share/applications/lazyboy-terminal.desktop
COPY --chmod=644 fonts.conf /etc/fonts/conf.d/99-lazyboy-cjk.conf
COPY --chmod=644 gtk3-settings.ini /etc/gtk-3.0/settings.ini
COPY --chmod=644 licenses/huninn-OFL.txt /usr/share/licenses/huninn/OFL.txt
# Hide the stock launchers so the menu shows only 瀏覽器 / 終端機 (box-chrome
# is the only supported way to start Chromium: same profile + CDP port).
RUN for f in chromium xfce4-terminal debian-xterm xterm uxterm; do \
      if [ -f "/usr/share/applications/${f}.desktop" ]; then \
        printf '\nNoDisplay=true\nHidden=true\n' >> "/usr/share/applications/${f}.desktop"; \
      fi; \
    done \
    && test -f /usr/share/locale/zh_TW/LC_MESSAGES/xfce4-panel.mo
# Traditional Chinese locale, Taipei time and AT-SPI on for every process in
# the box, including `docker exec` launches.
ENV DISPLAY=:1 \
    LANG=zh_TW.UTF-8 LC_ALL=zh_TW.UTF-8 LANGUAGE=zh_TW:zh:en TZ=Asia/Taipei \
    GTK_MODULES=atk-bridge GTK_A11Y=atspi GNOME_ACCESSIBILITY=1 NO_AT_BRIDGE=0
WORKDIR /workspace
EXPOSE 6080
LABEL lazyboy.box.revision=computer-use-10
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/start-desktop.sh"]
