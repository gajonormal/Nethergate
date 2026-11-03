import assert from "node:assert/strict";
import fs from "node:fs/promises";
import http from "node:http";

const { chromium } = await import(
  process.env.PLAYWRIGHT_MODULE ?? "playwright"
);
const css = await fs.readFile("ui/styles/pages/library.css", "utf8");
const logo = (await fs.readFile("ui/assets/nethergate-logo.svg")).toString(
  "base64",
);
const font = (await fs.readFile("ui/assets/ore/Minecraft.otf")).toString(
  "base64",
);
const server = http.createServer((req, res) => {
  res.setHeader("Content-Type", "text/html");
  res.end(`<style>@font-face{font-family:Minecraft;src:url(data:font/otf;base64,${font})}*{box-sizing:border-box}body{margin:0;font:14px Minecraft}.main-content{width:1000px}.button{padding:8px 14px;border:2px solid #444;font:inherit}${css}</style>
    <div class="library-workspace"><div class="main-content"><div class="page-heading">
      <div><h1 class="library-brand"><img class="nethergate-lockup" src="data:image/svg+xml;base64,${logo}"></h1></div>
      <div class="library-heading-actions"><button class="button">+ Novo servidor</button><button class="button">Importar servidor…</button></div>
    </div></div></div>`);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
let browser;
try {
  browser = await chromium.launch({
    headless: true,
    executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH,
  });
  const page = await browser.newPage({
    viewport: { width: 1200, height: 700 },
  });
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await page.evaluate(() => document.fonts.ready);
  for (const width of [1000, 760, 640, 540, 380]) {
    await page
      .locator(".main-content")
      .evaluate((el, w) => (el.style.width = `${w}px`), width);
    const [brand, first, second, group] = await Promise.all([
      page.locator(".nethergate-lockup").boundingBox(),
      page.locator(".button").nth(0).boundingBox(),
      page.locator(".button").nth(1).boundingBox(),
      page.locator(".library-heading-actions").boundingBox(),
    ]);
    assert(
      group.x >= brand.x + brand.width,
      `actions stay beside logo at ${width}`,
    );
    assert(
      Math.abs(brand.y + brand.height / 2 - group.y - group.height / 2) < 1,
      `vertical alignment at ${width}`,
    );
    assert(
      Math.abs(first.width - second.width) < 1,
      `equal button widths at ${width}`,
    );
    assert(
      second.x + second.width <= width + 1,
      `no right overflow at ${width}`,
    );
    if (width <= 760) {
      assert.equal(first.x, second.x);
      assert(
        second.y >= first.y + first.height + 11,
        `buttons stack on the right at ${width}`,
      );
    } else assert.equal(first.y, second.y);
  }
  console.log(
    "PASS: wide and compact library headers at five widths; actions stay beside logo, align centrally and do not overflow",
  );
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
