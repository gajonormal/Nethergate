import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import http from "node:http";
import { build } from "esbuild";
const { chromium } = await import(
  process.env.PLAYWRIGHT_MODULE ?? "playwright"
);
const mock = `export const desktop=false;
export async function openCloudPage(){};export async function openDuckDns(){};export async function openEula(){};export async function openPlayit(){};
export function scopedApi(scope){return(action,payload={})=>api(action,{...scope,...payload});}
window.requests=[];window.pendingLists=[];window.holdList=false;
const server={id:'one',name:'Test world',software:'paper',version:'1.21.11',memoryMb:2048,port:25565,contact:'https://example.org',javaMajor:21,javaPath:'auto',state:'offline',installed:true,eulaAccepted:true,error:null,pid:null,localAddress:'127.0.0.1:25565',directory:'fixture'};
window.fixture=server;
export async function api(action,payload){window.requests.push({action,payload});
if(action==='list'){if(window.holdList)return new Promise(resolve=>window.pendingLists.push(resolve));return [server];}
if(action==='energy')return {supported:false,activeServers:0,protectedServers:0,supply:null,error:null};
if(action==='logs'||action==='versions')return [];
if(action==='recovery')return {enabled:false,state:{attempt:0,maximumAttempts:3,remainingSeconds:null,pending:false}};
if(action==='gameplay')return {difficulty:'easy',gamemode:'survival',maxPlayers:20,viewDistance:10,simulationDistance:10,pvp:true};
if(action==='metrics')return {supported:false};
if(action==='world_preview')return {revision:null,image:null,generating:false};
throw Error('Unexpected fixture operation: '+action);}`;
const bundle = await build({
  stdin: {
    resolveDir: process.cwd(),
    loader: "tsx",
    contents: `import React,{useEffect,useState} from 'react';import{usePolling}from'./ui/hooks/usePolling';import{createRoot}from'react-dom/client';import{App}from'./ui/App';import{useServerLibrary}from'./ui/hooks/useServerLibrary';const root=createRoot(document.getElementById('root'));root.render(<App/>);function Probe(){const data=useServerLibrary(()=>{});useEffect(()=>{window.library=data;});return <output>{data.servers.map(s=>s.name).join(',')}</output>;}window.mountProbe=()=>root.render(<Probe/>);window.unmountProbe=()=>root.unmount();window.pendingReads=[];function PollProbe(){const[key,setKey]=useState('first');const[value,setValue]=useState('');useEffect(()=>{window.setPollKey=setKey;});usePolling(()=>new Promise(resolve=>window.pendingReads.push({key,resolve})),{interval:30,dependencies:[key],onData:setValue,onError:()=>{}});return <output>{value}</output>;}window.mountPollProbe=()=>root.render(<PollProbe/>);`,
  },
  bundle: true,
  write: false,
  format: "iife",
  jsx: "automatic",
  loader: { ".svg": "dataurl", ".png": "dataurl", ".css": "empty" },
  plugins: [
    {
      name: "mock-api",
      setup(b) {
        b.onResolve({ filter: /(^|\/)api$/ }, () => ({
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
    `<link rel="stylesheet" href="/assets/${css}"><div id="root"></div><script src="/app.js"></script>`,
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
    viewport: { width: 1320, height: 860 },
    reducedMotion: "reduce",
  });
  const errors = [];
  page.on("pageerror", (e) => {
    errors.push(e.message);
    console.error(e.message);
  });
  const snapshots = {};
  async function snapshot(name) {
    if (!process.env.STYLE_SNAPSHOT) return;
    await page.evaluate(() => document.fonts.ready);
    snapshots[name] = await page.locator(".main-content").evaluate((root) =>
      [root, ...root.querySelectorAll("*")].map((el) => {
        const style = getComputedStyle(el);
        return [
          el.tagName,
          el.className,
          ...[
            "display",
            "position",
            "width",
            "height",
            "padding",
            "margin",
            "gap",
            "color",
            "backgroundColor",
            "border",
            "borderRadius",
            "boxShadow",
            "fontFamily",
            "fontSize",
            "fontWeight",
            "overflow",
          ].map((k) => style[k]),
        ];
      }),
    );
  }
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await page
    .getByRole("heading", { name: "Test world", exact: true })
    .waitFor();
  await snapshot("library");
  await page.getByRole("searchbox").fill("missing");
  assert.equal(await page.locator(".server-card").count(), 0);
  await page.getByRole("button", { name: "Limpar pesquisa" }).click();
  await page
    .getByRole("button", { name: "Novo servidor", exact: true })
    .click();
  await page.getByRole("dialog").waitFor();
  await snapshot("dialog");
  await page.getByRole("dialog").locator(".modal-close").click();
  await page.getByRole("dialog").waitFor({ state: "detached" });
  await page.getByRole("button", { name: "Abrir", exact: true }).click();
  await page.waitForTimeout(200);
  assert.deepEqual(errors, []);
  await page.getByRole("textbox", { name: "Comando do servidor" }).waitFor();
  await snapshot("console");
  await page.getByRole("button", { name: "Definições", exact: true }).click();
  await page.getByRole("heading", { name: "Definições do servidor" }).waitFor();
  await snapshot("settings");
  if (process.env.STYLE_SNAPSHOT)
    await fs.writeFile(process.env.STYLE_SNAPSHOT, JSON.stringify(snapshots));
  // Exercise an old response arriving after a library switch.
  await page.evaluate(() => {
    window.holdList = true;
    window.mountProbe();
  });
  await page.waitForFunction(
    () => window.pendingLists.length === 1 && !!window.library,
  );
  await page.evaluate(() => {
    window.library.reset();
    void window.library.refresh();
  });
  await page.waitForFunction(() => window.pendingLists.length === 2);
  await page.evaluate(() =>
    window.pendingLists[1]([{ ...window.fixture, name: "New library" }]),
  );
  await page.getByText("New library", { exact: true }).waitFor();
  await page.evaluate(() =>
    window.pendingLists[0]([{ ...window.fixture, name: "Old library" }]),
  );
  await page.waitForTimeout(50);
  assert.equal(await page.locator("output").innerText(), "New library");
  await page.evaluate(() => {
    window.library.reset();
    void window.library.refresh();
  });
  await page.waitForFunction(() => window.pendingLists.length === 3);
  await page.evaluate(() => {
    window.mountPollProbe();
    window.pendingLists[2]([]);
  });
  await page.waitForFunction(() => window.pendingReads.length === 1);
  await page.waitForTimeout(80);
  assert.equal(
    await page.evaluate(() => window.pendingReads.length),
    1,
    "polls must not overlap",
  );
  await page.evaluate(() => window.setPollKey("second"));
  await page.waitForFunction(() => window.pendingReads.length === 2);
  await page.evaluate(() => window.pendingReads[1].resolve("Current scope"));
  await page.getByText("Current scope", { exact: true }).waitFor();
  await page.evaluate(() => window.pendingReads[0].resolve("Stale scope"));
  await page.waitForTimeout(50);
  assert.equal(await page.locator("output").innerText(), "Current scope");
  await page.waitForFunction(() => window.pendingReads.length === 3);
  await page.evaluate(() => {
    window.unmountProbe();
    window.pendingReads[2].resolve("Late result");
  });
  assert.deepEqual(errors, []);
  console.log(
    "PASS: actual app library, search, creation dialog, console/settings navigation; stale library responses ignored and shared polling prevents overlapping requests and ignores old scopes and unmounts.",
  );
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
