import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import solid from "vite-plugin-solid";

function reportDataPlugin(): Plugin {
  const modelPath = fileURLToPath(new URL("../results/normalized/report-model.json", import.meta.url));

  return {
    name: "evm-bench-report-data",
    configureServer(server) {
      server.middlewares.use(async (request, response, next) => {
        const pathname = request.url?.split("?")[0];
        if (pathname !== "/report-data.js" && pathname !== "/report-model.json") {
          next();
          return;
        }

        try {
          const model = await readFile(modelPath, "utf8");
          if (pathname === "/report-data.js") {
            response.setHeader("Content-Type", "application/javascript; charset=utf-8");
            response.end(`window.__EVM_BENCH_REPORT_DATA = ${model};\n`);
          } else {
            response.setHeader("Content-Type", "application/json; charset=utf-8");
            response.end(model);
          }
        } catch {
          response.statusCode = 404;
          response.setHeader("Content-Type", "text/plain; charset=utf-8");
          response.end(`Missing ${modelPath}. Run \`cargo run -- run\` from the repository root first.`);
        }
      });
    },
  };
}

export default defineConfig({
  base: "./",
  plugins: [reportDataPlugin(), solid()],
  build: {
    target: "es2022",
    sourcemap: true,
  },
});
