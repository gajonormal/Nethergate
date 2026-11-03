import assert from "node:assert/strict";
import { build } from "esbuild";
const { chromium } = await import(
  process.env.PLAYWRIGHT_MODULE ?? "playwright"
);

const image =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6lGkAAAAASUVORK5CYII=";
const bundle = await build({
  stdin: {
    resolveDir: process.cwd(),
    loader: "tsx",
    contents: `import React from 'react';import{createRoot}from'react-dom/client';import{WorldPreview}from'./ui/WorldPreview';const root=createRoot(document.getElementById('root'));window.requests=[];window.response={revision:null,image:null,generating:false};window.mount=id=>root.render(<WorldPreview key={id} id={id}/>);window.mount('one');`,
  },
  bundle: true,
  write: false,
  format: "iife",
  jsx: "automatic",
  plugins: [
    {
      name: "preview-api",
      setup(b) {
        b.onResolve({ filter: /(^|\/)api$/ }, () => ({
          path: "mock",
          namespace: "api",
        }));
        b.onLoad({ filter: /.*/, namespace: "api" }, () => ({
          contents:
            "export async function api(action,payload){window.requests.push({action,payload});return window.response;}",
          loader: "js",
        }));
      },
    },
  ],
});
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH,
});
try {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.clock.install();
  await page.setContent('<div id="root"></div>');
  await page.addScriptTag({ content: bundle.outputFiles[0].text });
  await page.waitForFunction(() => window.requests.length === 1);
  assert.equal(
    await page.locator("img").count(),
    0,
    "Ungenerated worlds keep the default cover",
  );
  await page.evaluate((image) => {
    window.response = { revision: "first", image, generating: false };
  }, image);
  await page.clock.fastForward(30_000);
  await page.locator(".world-preview").waitFor();
  assert.equal(await page.locator("img").getAttribute("src"), image);
  await page.evaluate(() => {
    window.response = { revision: "first", image: null, generating: false };
  });
  await page.clock.fastForward(30_000);
  await page.waitForFunction(() => window.requests.length >= 3);
  assert.equal(
    await page.locator("img").count(),
    1,
    "Unchanged revisions retain the current image",
  );
  assert.equal(
    await page.evaluate(() => window.requests.at(-1).payload.revision),
    "first",
  );
  await page.evaluate(() => {
    window.response = { revision: null, image: null, generating: false };
    window.mount("other");
  });
  await page.waitForFunction(
    () => window.requests.at(-1).payload.id === "other",
  );
  assert.equal(
    await page.locator("img").count(),
    0,
    "A different server must not inherit another world's cover",
  );
  assert.deepEqual(errors, []);
  console.log(
    "PASS: automatic covers load, reuse revisions and fall back without blocking the library.",
  );
} finally {
  await browser.close();
}
