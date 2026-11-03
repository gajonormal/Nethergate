import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import http from "node:http";

const { chromium } = await import(
  process.env.PLAYWRIGHT_MODULE ?? "playwright"
);
const assets = new Set(await fs.readdir("dist/assets"));
const cssFile = [...assets].find((name) => name.endsWith(".css"));
assert(cssFile, "Build the application before running the font test");
const server = http.createServer(async (req, res) => {
  const name = path.posix.basename(req.url.split("?")[0]);
  if (assets.has(name)) {
    res.setHeader(
      "Content-Type",
      name.endsWith(".css")
        ? "text/css"
        : name.endsWith(".otf")
          ? "font/otf"
          : "image/png",
    );
    res.end(await fs.readFile(path.join("dist/assets", name)));
  } else {
    res.setHeader("Content-Type", "text/html");
    res.end(`<html><head><link rel="stylesheet" href="/assets/${cssFile}"></head><body>
      <div class="address-bar"><strong id="ip">192.168.1.4:25565</strong></div>
      <code id="code">example.duckdns.org:25565</code><pre id="pre">Server ready</pre>
      <div class="console-output" id="logs">Minecraft server ready</div>
      <div class="console-input"><input id="command" value="say hello"></div>
      <span class="backup-size" id="size">19,3 MB</span>
      <span class="library-path" id="path">C:\\Nethergate\\server</span>
      <span class="firewall-path" id="firewall">C:\\Java\\java.exe</span>
    </body></html>`);
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
let browser;
try {
  browser = await chromium.launch({
    headless: true,
    executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH,
  });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const loaded = await page.evaluate(async () => {
    const faces = await document.fonts.load(
      "16px Minecraft",
      "192.168.1.4:25565",
    );
    return faces.length > 0 && faces.every((face) => face.status === "loaded");
  });
  assert(loaded, "Bundled Minecraft font must load");
  for (const id of [
    "ip",
    "code",
    "pre",
    "logs",
    "command",
    "size",
    "path",
    "firewall",
  ]) {
    const family = await page
      .locator(`#${id}`)
      .evaluate((el) => getComputedStyle(el).fontFamily);
    assert(family.startsWith("Minecraft"), `${id}: ${family}`);
  }
  const session = await page.context().newCDPSession(page);
  await session.send("DOM.enable");
  await session.send("CSS.enable");
  const { root } = await session.send("DOM.getDocument");
  const { nodeId } = await session.send("DOM.querySelector", {
    nodeId: root.nodeId,
    selector: "#ip",
  });
  const { fonts } = await session.send("CSS.getPlatformFontsForNode", {
    nodeId,
  });
  assert(
    fonts.length > 0 && fonts.every((font) => font.isCustomFont),
    "IP glyphs must render using the bundled font, not a system fallback",
  );
  console.log(
    "PASS: bundled Minecraft font loaded; IP glyphs use custom font; addresses, code, logs, commands, sizes and paths share its font family",
  );
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
