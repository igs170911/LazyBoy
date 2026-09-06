import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig({plugins:[react(),{name:"desktop-viewer",generateBundle(){this.emitFile({type:"asset",fileName:"vnc.html",source:readFileSync(new URL("./vnc.html",import.meta.url),"utf8")})}}],build:{outDir:"dist",emptyOutDir:true},server:{port:5173,proxy:{"/api":{target:"http://127.0.0.1:3101",ws:true},"/view":{target:"http://127.0.0.1:3101",ws:true}}}});
