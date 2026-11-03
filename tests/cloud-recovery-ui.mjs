import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import http from "node:http";
import { build } from "esbuild";
const { chromium } = await import(
  process.env.PLAYWRIGHT_MODULE ?? "playwright"
);
const mock = `window.calls=[]; window.pendingRestore=true;
const copy={id:'123-ab',createdAt:1760000000000,sizeBytes:1048576,fileCount:3,worlds:['world'],minecraftVersion:'1.21.11',reason:'automatic'};
let settings={endpoint:'https://s3.example.org',region:'us-east-1',bucket:'bucket',prefix:'nethergate/test',automatic:false,compression:false,retentionKeep:0};
const status=()=>({configured:true,busy:false,settings,state:{lastSuccess:null,lastBackupId:null,error:null,retentionError:null,completedFiles:0,totalFiles:0}});
export function scopedApi(scope){return(action,payload={})=>api(action,{...scope,...payload});}
export async function api(action,payload){window.calls.push({action,payload});
if(action==='restore_recovery_pending')return window.pendingRestore;
if(action==='restore_recovery_plan')return {fingerprint:'recovery-plan',safetyBackup:copy,sourceBackup:'456-ab',worlds:['world']};
if(action==='recover_restore'){window.pendingRestore=false;return {message:'Estado anterior reposto. Dados interrompidos preservados.'};}
if(action==='cloud_status')return status();
if(action==='cloud_save'){settings=payload.settings;return status();}
if(action==='cloud_retention_preview')return {fingerprint:'retention-plan',keep:payload.keep,remove:[copy]};
if(action==='cloud_retention_apply')return {removed:1};
if(action==='cloud_history')return {backups:[copy],limited:false};
throw new Error(action);}`;
const bundle = await build({
  stdin: {
    resolveDir: process.cwd(),
    loader: "tsx",
    contents: `import React from 'react';import{createRoot}from'react-dom/client';import{RestoreRecoveryPanel}from'./ui/RestoreRecoveryPanel';import{CloudBackupPanel}from'./ui/CloudBackupPanel';import{Disclosure}from'./ui/Disclosure';const server={id:'test',state:'offline',pid:null};createRoot(document.getElementById('root')).render(<main className="world-panel"><RestoreRecoveryPanel server={server} onRecovered={()=>{window.recovered=true}}/><div className="world-options"><Disclosure title="Backups cloud"><CloudBackupPanel server={server} onHistoryChange={()=>{}}/></Disclosure></div></main>);`,
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
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await page
    .getByRole("button", { name: "Verificar cópia de recuperação" })
    .click();
  const recover = page.getByRole("button", { name: "Recuperar mundo" });
  assert(await recover.isDisabled());
  await page
    .getByRole("switch", { name: "Repor o estado anterior ao restauro" })
    .check();
  await recover.click();
  await page
    .getByText("Estado anterior reposto. Dados interrompidos preservados.")
    .waitFor();
  assert(await page.evaluate(() => window.recovered));
  const call = await page.evaluate(() =>
    window.calls.find((c) => c.action === "recover_restore"),
  );
  assert.deepEqual(call.payload, {
    id: "test",
    fingerprint: "recovery-plan",
    confirmed: true,
  });
  await page.getByText("Backups cloud", { exact: true }).click();
  await page.getByText("Configurar destino S3", { exact: true }).click();
  await page
    .getByRole("switch", { name: "Comprimir novos envios com gzip" })
    .check();
  await page
    .getByLabel("Retenção automática cloud", { exact: true })
    .selectOption("7");
  await page.getByRole("button", { name: "Guardar destino cloud" }).click();
  const save = await page.evaluate(() =>
    window.calls.find((c) => c.action === "cloud_save"),
  );
  assert.equal(save.payload.settings.compression, true);
  assert.equal(save.payload.settings.retentionKeep, 7);
  await page.getByText("Configurar destino S3", { exact: true }).click();
  await page
    .getByText("Limpar cópias automáticas antigas", { exact: true })
    .click();
  await page.getByRole("button", { name: "Rever limpeza" }).click();
  const apply = page.getByRole("button", { name: "Aplicar limpeza" });
  assert(await apply.isDisabled());
  await page
    .getByRole("switch", { name: "Remover estas cópias da cloud" })
    .check();
  for (const width of [1200, 720]) {
    await page.setViewportSize({ width, height: 900 });
    assert(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
      `overflow at ${width}`,
    );
    for (const b of await page.locator("button:visible").all()) {
      const box = await b.boundingBox();
      assert(
        box.x >= 0 && box.x + box.width <= width,
        `button clipped at ${width}`,
      );
    }
  }
  await page.setViewportSize({ width: 1200, height: 900 });
  await fs.mkdir(".tmp/ui-captures", { recursive: true });
  await page.screenshot({
    path: ".tmp/ui-captures/cloud-recovery.png",
    fullPage: true,
  });
  await apply.click();
  await page
    .getByText("Limpeza concluída: 1 cópias automáticas removidas.")
    .waitFor();
  const cleanup = await page.evaluate(() =>
    window.calls.find((c) => c.action === "cloud_retention_apply"),
  );
  assert.deepEqual(cleanup.payload, {
    id: "test",
    keep: 7,
    fingerprint: "retention-plan",
    confirmed: true,
  });
  console.log(
    "PASS: recovery and retention require confirmation; exact plans submitted; gzip and retention settings saved; controls fit desktop and compact widths.",
  );
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
