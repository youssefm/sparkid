import { defineConfig } from "tsup";
import path from "path";

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["cjs", "esm"],
  dts: true,
  sourcemap: true,
  clean: true,
  esbuildOptions(options) {
    // Rewrite absolute source paths to relative so local filesystem
    // paths don't leak into the published npm package.
    options.sourceRoot = path.relative(
      path.resolve(__dirname, "dist"),
      path.resolve(__dirname),
    );
  },
});
