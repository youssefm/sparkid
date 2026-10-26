import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/index.ts", "src/binary.ts"],
  format: ["cjs", "esm"],
  dts: true,
  // esbuild already emits map `sources` relative to the map file (e.g.
  // "../src/index.ts"), so no local paths leak and they resolve to the shipped
  // src/. Don't set `sourceRoot`: consumers prepend it to those paths, so stack
  // traces and debuggers point at nonexistent files (e.g. "dist/..../src/").
  sourcemap: true,
  clean: true,
});
