import assert from "node:assert/strict";
import fs from "node:fs/promises";
import http from "node:http";

const { chromium } = await import(
  process.env.PLAYWRIGHT_MODULE ?? "playwright"
);
const css = await fs.readFile("ui/styles/controls/buttons.css", "utf8");
const server = http.createServer((req, res) => {
  res.setHeader("Content-Type", "text/html");
  res.end(`<style>${css}body{padding:24px;font:20px sans-serif}div{margin:20px 0}</style>
    <div id="label">InterfaceLabel</div><code id="address">192.168.1.4:25565</code>
    <div class="console-output"><span>DiagnosticLog</span></div>
    <input value="Editable command"><textarea>Editable notes</textarea>
    <div contenteditable="true">Editable document</div>
    <button onclick="this.textContent='Copied'">Copy</button>`);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
let browser;
try {
  browser = await chromium.launch({
    headless: true,
    executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH,
  });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  for (const selector of ["#label", "#address"]) {
    await page.locator(selector).dblclick();
    assert.equal(
      await page.evaluate(() => window.getSelection().toString()),
      "",
      `no double-click selection on ${selector}`,
    );
  }
  const label = await page.locator("#label").boundingBox();
  await page.mouse.move(label.x + 2, label.y + label.height / 2);
  await page.mouse.down();
  await page.mouse.move(label.x + 130, label.y + label.height / 2, {
    steps: 8,
  });
  await page.mouse.up();
  assert.equal(
    await page.evaluate(() => window.getSelection().toString()),
    "",
    "no drag selection on UI labels",
  );
  await page.locator(".console-output span").dblclick();
  assert.equal(
    await page.evaluate(() => window.getSelection().toString()),
    "DiagnosticLog",
  );
  for (const selector of ["input", "textarea"]) {
    await page.locator(selector).click();
    await page.locator(selector).press("ControlOrMeta+A");
    assert.equal(
      await page
        .locator(selector)
        .evaluate((el) => el.selectionEnd - el.selectionStart),
      await page.locator(selector).evaluate((el) => el.value.length),
    );
  }
  await page.locator("[contenteditable]").dblclick();
  assert(
    ["Editable", "document"].includes(
      await page.evaluate(() => window.getSelection().toString()),
    ),
  );
  await page.getByText("Copy", { exact: true }).click();
  assert.equal(await page.locator("button").textContent(), "Copied");
  console.log(
    "PASS: UI double-click/drag selection blocked; console, inputs and editor selectable; copy button clickable",
  );
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
