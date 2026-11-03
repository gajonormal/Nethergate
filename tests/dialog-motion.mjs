import assert from "node:assert/strict";
import fs from "node:fs/promises";
import http from "node:http";
import { build } from "esbuild";

// Use a locally available Playwright installation; no downloads or app data.
const { chromium } = await import(
  process.env.PLAYWRIGHT_MODULE ?? "playwright"
);
const bundle = await build({
  stdin: {
    resolveDir: process.cwd(),
    loader: "tsx",
    contents: `import React, {useState} from 'react';
      import {createRoot} from 'react-dom/client';
      import {createPortal} from 'react-dom';
      import {DialogPresence, DialogBackdrop} from './ui/DialogPresence';
      import {ApplicationMenu} from './ui/ApplicationMenu';
      function App(){
        const [open,setOpen]=useState(false);
        const [count,setCount]=useState(0);
        window.reopen=()=>setOpen(true);
        return <><button onClick={()=>setOpen(true)}>Open</button><output>{count}</output>
          <DialogPresence>{open && createPortal(<DialogBackdrop className="modal-backdrop">
            <section className="modal" role="dialog"><h2>Dialog</h2>
              <button className="button" onClick={()=>setCount(x=>x+1)}>Action</button>
              <button className="modal-close" onClick={()=>setOpen(false)}>Close</button>
            </section></DialogBackdrop>,document.body)}</DialogPresence>
          <div className="app-shell"><div className="side-controls"><ApplicationMenu>
              <button className="nav-item">Notifications</button><button className="nav-item">Library</button>
            </ApplicationMenu></div></div></>;
      }createRoot(document.getElementById('root')).render(<App/>);`,
  },
  bundle: true,
  write: false,
  format: "iife",
  jsx: "automatic",
});
const nether = await fs.readFile("ui/styles/theme/nether.css", "utf8");
const menuTexture = nether.match(/\.creative-tab\.active,[\s\S]*?\n\}/)[0];
const css =
  `:root{--netherrack:url('/netherrack.png')}.app-options-content{width:220px}.nav-item{display:block;width:100%;height:44px}` +
  menuTexture +
  (await fs.readFile("ui/styles/components/dialogs.css", "utf8")) +
  (await fs.readFile("ui/styles/motion.css", "utf8"));
const netherrack = await fs.readFile("ui/assets/ore/netherrack.png");
const server = http.createServer((req, res) => {
  if (req.url === "/app.js") {
    res.setHeader("Content-Type", "text/javascript");
    res.end(bundle.outputFiles[0].text);
  } else if (req.url === "/netherrack.png") {
    res.setHeader("Content-Type", "image/png");
    res.end(netherrack);
  } else {
    res.setHeader("Content-Type", "text/html");
    res.end(
      `<html><head><style>${css}</style></head><body><div id="root"></div><script src="/app.js"></script></body></html>`,
    );
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
  const url = `http://127.0.0.1:${server.address().port}`;
  await page.goto(url);
  await page.getByText("Open", { exact: true }).click();
  assert.equal(
    await page
      .locator(".modal")
      .evaluate((el) => getComputedStyle(el).animationName),
    "modal-enter",
  );
  const entryOpacity = await page.locator(".modal").evaluate((el) => {
    const animation = el.getAnimations()[0];
    animation.pause();
    const values = [0, 80, 160].map((time) => {
      animation.currentTime = time;
      return Number(getComputedStyle(el).opacity);
    });
    animation.finish();
    return values;
  });
  assert.equal(entryOpacity[0], 0);
  assert(entryOpacity[1] > 0 && entryOpacity[1] < 1);
  assert.equal(entryOpacity[2], 1);
  await page.getByText("Close", { exact: true }).click();
  assert.equal(
    await page.locator(".modal-backdrop").getAttribute("data-closing"),
    "true",
  );
  assert.equal(
    await page
      .locator(".modal")
      .evaluate((el) => getComputedStyle(el).animationName),
    "modal-exit",
  );
  const exitOpacity = await page.locator(".modal").evaluate((el) => {
    const animation = el.getAnimations()[0];
    animation.pause();
    return [0, 70, 140].map((time) => {
      animation.currentTime = time;
      return Number(getComputedStyle(el).opacity);
    });
  });
  assert.equal(exitOpacity[0], 1);
  assert(exitOpacity[1] > 0 && exitOpacity[1] < 1);
  assert.equal(exitOpacity[2], 0);
  await page.getByText("Action", { exact: true }).evaluate((el) => el.click());
  assert.equal(
    await page.locator("output").textContent(),
    "0",
    "closing dialog must block actions",
  );
  await page.locator(".modal").waitFor({ state: "detached" });

  await page.getByText("Open", { exact: true }).click();
  await page.getByText("Close", { exact: true }).click();
  await page.evaluate(() => window.reopen());
  await page.waitForTimeout(200);
  assert.equal(
    await page.locator(".modal").count(),
    1,
    "reopening cancels exit timer",
  );
  assert.equal(
    await page.locator(".modal-backdrop").getAttribute("data-closing"),
    null,
  );

  await page.locator(".app-options > summary").click();
  const revealFrames = await page
    .locator(".app-options-reveal")
    .evaluate((el) => {
      const animation = el.getAnimations()[0];
      animation.pause();
      const heights = [0, 80, 160].map((time) => {
        animation.currentTime = time;
        return el.getBoundingClientRect().height;
      });
      animation.currentTime = 120;
      return heights;
    });
  assert.equal(revealFrames[0], 0);
  assert(revealFrames[1] > 0 && revealFrames[1] < revealFrames[2]);
  assert.equal(
    await page
      .locator(".app-options-content")
      .evaluate((el) => getComputedStyle(el).transform),
    "none",
  );
  const textureDuring = await page
    .getByText("Notifications", { exact: true })
    .screenshot({ animations: "allow" });
  await page
    .locator(".app-options-reveal")
    .evaluate((el) => el.getAnimations()[0].finish());
  const textureAfter = await page
    .getByText("Notifications", { exact: true })
    .screenshot({ animations: "allow" });
  assert(
    textureDuring.equals(textureAfter),
    "netherrack pixels must remain identical during and after menu reveal",
  );

  await page.locator(".app-options > summary").click();
  assert.equal(
    await page.locator(".app-options").getAttribute("data-closing"),
    "true",
  );
  assert.equal(
    await page.locator(".app-options").evaluate((el) => el.open),
    true,
    "menu remains open while hiding",
  );
  const hideFrames = await page
    .locator(".app-options-reveal")
    .evaluate((el) => {
      const animation = el.getAnimations()[0];
      animation.pause();
      return [0, 80, 160].map((time) => {
        animation.currentTime = time;
        return el.getBoundingClientRect().height;
      });
    });
  assert(hideFrames[0] > hideFrames[1] && hideFrames[1] > 0);
  assert.equal(hideFrames[2], 0);
  await page
    .locator(".app-options-reveal")
    .evaluate((el) => el.getAnimations()[0].finish());
  await page.waitForFunction(
    () => !document.querySelector(".app-options").open,
  );

  await page.locator(".app-options > summary").click();
  await page
    .locator(".app-options-reveal")
    .evaluate((el) => el.getAnimations()[0]?.finish());
  await page.locator(".app-options > summary").click();
  const reversal = await page
    .locator(".app-options-reveal")
    .evaluate(async (el) => {
      const current = el.getAnimations()[0];
      current.pause();
      current.currentTime = 80;
      const before = el.getBoundingClientRect().height;
      el.parentElement.querySelector("summary").click();
      await new Promise(requestAnimationFrame);
      return [before, el.getBoundingClientRect().height];
    });
  assert(
    Math.abs(reversal[0] - reversal[1]) < 10,
    "reversing midway must continue from the current height",
  );
  await page.waitForTimeout(200);
  assert.equal(
    await page.locator(".app-options").evaluate((el) => el.open),
    true,
    "rapid menu reopen cancels closing",
  );
  assert.equal(
    await page.locator(".app-options").getAttribute("data-closing"),
    null,
  );

  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.locator(".app-options > summary").press("Enter");
  assert.equal(
    await page.locator(".app-options").evaluate((el) => el.open),
    false,
    "keyboard close respects reduced motion",
  );
  await page.getByText("Close", { exact: true }).click();
  await page.locator(".modal").waitFor({ state: "detached" });
  await page.getByText("Open", { exact: true }).click();
  assert.equal(
    await page
      .locator(".modal")
      .evaluate((el) => getComputedStyle(el).animationName),
    "none",
  );
  console.log(
    "PASS: popup fades, menu reveal/hide, stable texture pixels, rapid reopen, keyboard and reduced motion",
  );
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
