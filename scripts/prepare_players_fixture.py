"""Isolated Paper runtime for player-management tests. Does not copy user worlds."""
import json
import shutil
import uuid
from pathlib import Path

project = Path(__file__).resolve().parents[1]
source = project / "server"
root = project / "hub-data" / f"players-ui-test-{uuid.uuid4().hex[:8]}"
server = root / "server"
server.mkdir(parents=True)
config = json.loads((source / "nethergate.json").read_text(encoding="utf-8-sig"))
config.update(port=25591, memory_mb=2048)
(server / "nethergate.json").write_text(json.dumps(config), encoding="utf-8")
for name in ("paper.jar", "paper.lock.json", "eula.txt", "usercache.json"):
    shutil.copy2(source / name, server / name)
for name in ("libraries", "versions", "cache"):
    shutil.copytree(source / name, server / name)
(server / "server.properties").write_text(
    "server-ip=127.0.0.1\nonline-mode=true\nwhite-list=false\n"
    "enforce-whitelist=false\nview-distance=3\nsimulation-distance=3\n"
    "level-name=world\nlevel-seed=12345\nmax-players=4\n", encoding="utf-8"
)
(root / "hub-data").mkdir()
(root / "hub-data" / "registry.json").write_text(json.dumps([
    {"id": "fixture", "name": "Validação de jogadores (teste)", "directory": str(server)}
]), encoding="utf-8")
shutil.copytree(project / "dist", root / "dist")
(project / "hub-data" / "players-test-location.txt").write_text(str(root), encoding="utf-8")
print(root)
