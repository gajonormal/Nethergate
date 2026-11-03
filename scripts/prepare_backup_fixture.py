"""Create disposable synthetic world files for UI backup/restore tests; never starts Java."""
import json
import shutil
import uuid
from pathlib import Path

project = Path(__file__).resolve().parents[1]
root = project / "hub-data" / f"backup-ui-test-{uuid.uuid4().hex[:8]}"
server = root / "server"
server.mkdir(parents=True)
(server / "nethergate.json").write_text(json.dumps({
    "minecraft": "1.21.11", "contact": "https://example.org", "java": "java",
    "java_major": 21, "memory_mb": 2048, "port": 25590,
}), encoding="utf-8")
(server / "server.properties").write_text("level-name=world\nserver-ip=127.0.0.1\n", encoding="utf-8")
for name in ("world", "world_nether", "world_the_end"):
    world = server / name
    (world / "region").mkdir(parents=True)
    (world / "level.dat").write_bytes(f"SYNTHETIC-ORIGINAL-{name}".encode())
    (world / "region" / "r.0.0.mca").write_bytes(f"SYNTHETIC-CHUNKS-{name}".encode())
(root / "hub-data").mkdir()
(root / "hub-data" / "registry.json").write_text(json.dumps([
    {"id": "fixture", "name": "Validação de backups (teste)", "directory": str(server)},
]), encoding="utf-8")
shutil.copytree(project / "dist", root / "dist")
(project / "hub-data" / "backup-test-location.txt").write_text(str(root), encoding="utf-8")
print(root)
