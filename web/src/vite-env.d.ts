/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly LAZYBOY_VERSION: string;
  readonly LAZYBOY_BUILD: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

