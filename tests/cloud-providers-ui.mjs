import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import http from "node:http";
import { build } from "esbuild";
const { chromium } = await import(
  process.env.PLAYWRIGHT_MODULE ?? "playwright"
);
const mock = `window.calls=[];window.opened=[];
window.accounts={google_drive:false,dropbox:false};window.configs={google_drive:{namespace:'test',compression:true,automatic:false},dropbox:{namespace:'test',compression:true,automatic:false}};
const copy={id:'123-ab',createdAt:1760000000000,sizeBytes:1048576,fileCount:2,worlds:['world'],minecraftVersion:'1.21.11',reason:'manual'};
const status=p=>({busy:false,account:{connected:window.accounts[p],clientId:null,pending:false,error:null},settings:window.configs[p],state:{error:null,lastSuccess:null,sentBlocks:2,reusedBlocks:4,pendingBackup:'123-ab'}});
export async function openCloudPage(url){window.opened.push(url);}
export function scopedApi(scope){return(action,payload={})=>api(action,{...scope,...payload});}
export async function api(action,payload){window.calls.push({action,payload});
if(action==='cloud_status')return {configured:false,busy:false,settings:null,state:{}};
if(action==='service_cloud_status')return status(payload.provider);
if(action==='cloud_account_begin'){window.accounts[payload.provider]=true;return {url:payload.provider==='google_drive'?'https://accounts.google.com/o/oauth2/v2/auth?state=test':'https://www.dropbox.com/oauth2/authorize?state=test'};}
if(action==='cloud_account_disconnect'){window.accounts[payload.provider]=false;return {};}
if(action==='service_cloud_save'){window.configs[payload.provider]=payload.settings;return status(payload.provider);}
if(action==='service_cloud_history')return {backups:[copy],limited:false};
if(action==='service_cloud_upload'||action==='service_cloud_download')return {started:true};
throw new Error(action);}`;
const bundle = await build({
  stdin: {
    resolveDir: process.cwd(),
    loader: "tsx",
    contents: `import React from 'react';import{createRoot}from'react-dom/client';import{CloudProvidersPanel}from'./ui/CloudProvidersPanel';import{Disclosure}from'./ui/Disclosure';const server={id:'test',state:'offline',pid:null};createRoot(document.getElementById('root')).render(<main className="world-panel"><div className="world-options"><Disclosure title="Backups cloud"><CloudProvidersPanel server={server} onHistoryChange={()=>{}}/></Disclosure></div></main>);`,
  },
  plugins: [
    {
      name: "isolated-api",
      setup(b) {
        b.onResolve({ filter: /^\.\/api$/ }, () => ({
          path: "mock",
          namespace: "api",
        }));
        b.onLoad({ filter: /.*/, namespace: "api" }, () => ({
          contents: mock,
          loader: "js",
        }));
      },
    },
  ],
  bundle: true,
  write: false,
  format: "iife",
  jsx: "automatic",
  loader: { ".svg": "dataurl", ".png": "dataurl", ".css": "empty" },
});
const assets = new Set(await fs.readdir("dist/assets"));
const css = [...assets].find((n) => n.endsWith(".css"));
const server = http.createServer(async (req, res) => {
  if (req.url === "/app.js") {
    res.setHeader("Content-Type", "text/javascript");
    return res.end(bundle.outputFiles[0].text);
  }
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
    return res.end(await fs.readFile(path.join("dist/assets", name)));
  }
  res.setHeader("Content-Type", "text/html");
  res.end(
    `<html><head><link rel="stylesheet" href="/assets/${css}"><style>body{background:#0c0c0c}main{max-width:1100px;margin:auto}.world-options{display:grid}</style></head><body><div id="root"></div><script src="/app.js"></script></body></html>`,
  );
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
let browser;
try {
  browser = await chromium.launch({
    headless: true,
    executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH,
  });
  const page = await browser.newPage({
    viewport: { width: 1200, height: 900 },
  });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await page.getByText("Backups cloud", { exact: true }).click();
  assert(
    await page
      .getByRole("button", { name: "Ligar conta", exact: true })
      .isDisabled(),
  );
  await page
    .getByLabel("Client ID (aplicação Desktop)", { exact: true })
    .fill("example.apps.googleusercontent.com");
  await page.getByRole("button", { name: "Ligar conta", exact: true }).click();
  assert(
    (await page.evaluate(() => window.opened[0])).startsWith(
      "https://accounts.google.com/",
    ),
  );
  await page.getByText("Preferências dos backups", { exact: true }).click();
  await page
    .getByLabel("Identificador cloud deste servidor", { exact: true })
    .fill("portfolio-world");
  await page
    .getByRole("switch", { name: "Enviar automaticamente com o motor ligado" })
    .check();
  await page
    .getByRole("button", { name: "Guardar preferências", exact: true })
    .click();
  const saved = await page.evaluate(() =>
    window.calls.find((c) => c.action === "service_cloud_save"),
  );
  assert.equal(saved.payload.provider, "google_drive");
  assert.equal(saved.payload.settings.namespace, "portfolio-world");
  assert.equal(saved.payload.settings.automatic, true);
  await page
    .getByRole("button", { name: "Retomar envio interrompido", exact: true })
    .click();
  const resumed = await page.evaluate(() =>
    window.calls.find((c) => c.action === "service_cloud_upload"),
  );
  assert.equal(resumed.payload.backupId, "123-ab");
  await page
    .getByRole("button", { name: "Consultar histórico", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Transferir para backups", exact: true })
    .click();
  assert(
    (await page.evaluate(() => window.calls)).some(
      (c) =>
        c.action === "service_cloud_download" &&
        c.payload.backupId === "123-ab",
    ),
  );
  await page.getByRole("button", { name: "Dropbox", exact: true }).click();
  await page
    .getByLabel("App Key (aplicação App folder)", { exact: true })
    .fill("dropboxappkey");
  await page.getByRole("button", { name: "Ligar conta", exact: true }).click();
  assert(
    (await page.evaluate(() => window.opened[1])).startsWith(
      "https://www.dropbox.com/",
    ),
  );
  await page
    .getByRole("button", { name: "Consultar histórico", exact: true })
    .click();
  for (const width of [1200, 720]) {
    await page.setViewportSize({ width, height: 900 });
    assert(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
      `overflow at ${width}`,
    );
    for (const button of await page.locator("button:visible").all()) {
      const b = await button.boundingBox();
      assert(b.x >= 0 && b.x + b.width <= width, `button clipped at ${width}`);
    }
  }
  await page.setViewportSize({ width: 1200, height: 900 });
  await fs.mkdir(".tmp/ui-captures", { recursive: true });
  await page.screenshot({
    path: ".tmp/ui-captures/cloud-providers.png",
    fullPage: true,
  });
  await page
    .getByRole("button", { name: "S3 compatível", exact: true })
    .click();
  await page.getByText("Configurar destino S3", { exact: true }).click();
  assert(
    await page
      .getByRole("switch", {
        name: "Deduplicação e retoma por blocos após reiniciar",
      })
      .isChecked(),
  );
  assert.deepEqual(errors, []);
  console.log(
    "PASS: OAuth setup and browser handoff, provider-specific settings, exact resume/download IDs, and responsive controls for Drive/Dropbox/S3.",
  );
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
