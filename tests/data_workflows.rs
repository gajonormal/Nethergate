use nethergated::{
    backup_schedule::Policy,
    hub::{self, CreateServer, Hub},
};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

fn root() -> PathBuf {
    let mut random = [0u8; 16];
    getrandom::getrandom(&mut random).unwrap();
    let key: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let root = std::env::temp_dir().join(format!("ng-data-workflows-{key}"));
    fs::create_dir(&root).unwrap();
    root
}
fn input() -> CreateServer {
    serde_json::from_value(json!({"name":"Source","software":"paper","version":"1.21.11","memoryMb":1024,"port":25580,"contact":"https://example.org","javaMajor":21,"javaPath":"auto","eulaAccepted":true})).unwrap()
}
#[test]
fn hub_external_schedule_and_cross_library_transfer_preserve_world_and_reset_network() {
    let root = root();
    let hub = Hub::new(root.clone()).unwrap();
    let id = hub.create(input()).unwrap();
    let server = root.join("servers").join(&id);
    fs::create_dir(server.join("world")).unwrap();
    fs::write(server.join("world/level.dat"), b"synthetic-source-world").unwrap();
    let external = root.join("external");
    fs::create_dir(&external).unwrap();
    let destination = hub::dispatch(
        &hub,
        "save_backup_destination",
        json!({"id":id,"path":external}),
    )
    .unwrap();
    let store = PathBuf::from(destination["path"].as_str().unwrap());
    let manual = hub.create_backup(&id).unwrap();
    assert!(store.join(manual.id).is_dir());
    hub.save_backup_schedule(
        &id,
        Policy {
            enabled: true,
            interval_minutes: 15,
            keep: 2,
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if hub
            .backups(&id)
            .unwrap()
            .iter()
            .any(|copy| copy.reason == "automatic")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Automatic backup was not published externally"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    hub.save_backup_schedule(&id, Policy::default()).unwrap();
    fs::rename(&external, root.join("disconnected")).unwrap();
    assert!(hub.backups(&id).is_err());
    assert!(!external.exists());
    let packages = root.join("packages");
    fs::create_dir(&packages).unwrap();
    let exported = hub.export_server(&id, &packages).unwrap();
    let package = PathBuf::from(exported["path"].as_str().unwrap());
    let review = nethergated::transfer::inspect(&package).unwrap();
    let peer_root = root.join("peer");
    fs::create_dir(&peer_root).unwrap();
    let peer = Hub::new(peer_root.clone()).unwrap();
    let imported=peer.import_server(serde_json::from_value(json!({"path":package,"fingerprint":review["fingerprint"],"name":"Imported","port":25581,"memoryMb":1024,"contact":"https://new.example.org","javaPath":"auto","eulaAccepted":true})).unwrap()).unwrap();
    let imported_directory = peer_root.join("servers").join(&imported);
    assert_eq!(
        fs::read(imported_directory.join("world/level.dat")).unwrap(),
        b"synthetic-source-world"
    );
    assert_eq!(
        fs::read(server.join("world/level.dat")).unwrap(),
        b"synthetic-source-world"
    );
    assert!(
        !nethergated::backup_destination::status(&peer_root, &imported).unwrap()["external"]
            .as_bool()
            .unwrap()
    );
    let props = fs::read_to_string(imported_directory.join("server.properties")).unwrap();
    assert!(props.contains("server-ip=127.0.0.1"));
    assert!(props.contains("white-list=true"));
    assert!(props.contains("server-port=25581"));
    assert_eq!(peer.list().unwrap().len(), 1);
    assert_eq!(hub.list().unwrap().len(), 1);
    assert!(peer.import_server(serde_json::from_value(json!({"path":package,"fingerprint":review["fingerprint"],"name":"Duplicate port","port":25581,"memoryMb":1024,"contact":"https://example.org","javaPath":"auto","eulaAccepted":true})).unwrap()).is_err());
    assert_eq!(peer.list().unwrap().len(), 1);
}
