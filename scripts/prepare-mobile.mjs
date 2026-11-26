import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";

const root = process.cwd();
const webDir = path.join(root, "www");
const required = [
  path.join(root, "miniapp", "index.html"),
  path.join(root, "miniapp", "app.js"),
  path.join(root, "data", "groups.json"),
  path.join(root, "favicon.png"),
];

for (const file of required) {
  if (!existsSync(file)) throw new Error(`Required mobile asset is missing: ${path.relative(root, file)}`);
}

await rm(webDir, { recursive: true, force: true });
await mkdir(path.join(webDir, "miniapp"), { recursive: true });
await cp(path.join(root, "miniapp"), path.join(webDir, "miniapp"), { recursive: true });
await cp(path.join(root, "data"), path.join(webDir, "data"), { recursive: true });
await cp(path.join(root, "favicon.png"), path.join(webDir, "favicon.png"));

// Bundle the native-only Capacitor bridge into the Android app. The website keeps
// a harmless stub so the same Mini App source continues to work in browsers.
const { execFile } = await import("node:child_process");
const { promisify } = await import("node:util");
await promisify(execFile)("npx", [
  "esbuild",
  path.join(root, "mobile", "bridge.js"),
  "--bundle",
  "--format=iife",
  "--outfile=" + path.join(webDir, "miniapp", "native-bridge.js"),
], { cwd: root });

const entry = `<!doctype html>
<html lang="ru">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
  <meta name="theme-color" content="#173d2e">
  <meta http-equiv="refresh" content="0; url=./miniapp/index.html">
  <title>SPBFTUIT</title>
</head>
<body>
  <p>Открываем SPBFTUIT… <a href="./miniapp/index.html">Продолжить</a></p>
  <script>location.replace("./miniapp/index.html" + location.search + location.hash);</script>
</body>
</html>
`;
await writeFile(path.join(webDir, "index.html"), entry, "utf8");

const groups = JSON.parse(await readFile(path.join(webDir, "data", "groups.json"), "utf8"));
if (!Array.isArray(groups) && (!groups || typeof groups !== "object")) {
  throw new Error("data/groups.json has an unexpected structure");
}
console.log(`Prepared Android web assets; groups source loaded (${Array.isArray(groups) ? groups.length : Object.keys(groups).length} entries).`);
