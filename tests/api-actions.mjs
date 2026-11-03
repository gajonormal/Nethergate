import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
const rust = fs.readFileSync("src/hub/commands.rs", "utf8");
const actions = new Set();
for (const match of rust.matchAll(
  /^\s*((?:"[a-z_]+"\s*\|\s*)*"[a-z_]+")\s*=>/gm,
)) {
  for (const literal of match[1].matchAll(/"([a-z_]+)"/g))
    actions.add(literal[1]);
}
const source = ts.createSourceFile(
  "contract.ts",
  fs.readFileSync("ui/api/contract.ts", "utf8"),
  ts.ScriptTarget.Latest,
  true,
);
const contract = source.statements.find(
  (n) => ts.isInterfaceDeclaration(n) && n.name.text === "ApiContract",
);
for (const member of contract.members)
  assert(
    actions.has(member.name.getText(source)),
    `Unknown Rust action: ${member.name.getText(source)}`,
  );
const models = fs.readFileSync("ui/api/models.ts", "utf8");
const states = [
  ...models
    .slice(0, models.indexOf("export interface Server"))
    .matchAll(/"([a-z_]+)"/g),
].map((m) => m[1]);
const stateSource = fs.readFileSync("src/hub/state.rs", "utf8");
for (const state of states)
  assert(
    stateSource.includes(`=> "${state}"`),
    `Frontend state missing from Rust: ${state}`,
  );
assert.equal(
  states.length,
  [...stateSource.matchAll(/Self::\w+ => "/g)].length,
  "Both sides must describe the same lifecycle states",
);
console.log(
  `PASS: ${contract.members.length} frontend actions exist in Rust; ${states.length} lifecycle states agree across both sides.`,
);
