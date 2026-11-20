use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use proto_core::*;
use serde_json::{json, Value};

/// A fresh temp dir per test (removed by `Tmp`'s drop).
struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!("proto-core-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn design(id: &str, space: &str) -> Value {
    json!({"id": id, "spaceId": space, "kind": "page", "title": format!("T {id}"), "rationale": "", "html": "<p>x</p>",
           "status": "ready", "position": {"x": 1, "y": 2}, "createdAt": 1})
}

fn meta() -> ChangeMeta {
    ChangeMeta::client("test").reason("unit")
}

#[test]
fn paths() {
    assert!(is_valid_path("designs/abc"));
    assert!(is_valid_path("promotedFrom/x-1"));
    assert!(!is_valid_path("designs/"));
    assert!(!is_valid_path("/abc"));
    assert!(!is_valid_path("jobs/abc"));
    assert!(!is_valid_path("designs"));
    assert!(!is_valid_path(&format!("designs/{}", "a".repeat(600))));
    assert_eq!(split_path("spokeFocus/a/b"), Some((RecordKind::SpokeFocus, "a/b")));
    for k in RecordKind::ALL {
        assert_eq!(RecordKind::parse(k.as_str()), Some(k));
    }
}

#[test]
fn apply_versions_and_read_since() {
    let store = Store::in_memory();
    let c0 = store.cursor();
    assert_eq!(c0.version, 0);
    // Empty batch: cursor unchanged.
    assert_eq!(store.apply(vec![], meta()).unwrap(), c0);

    let c1 = store.apply(vec![Change::set("designs/a", design("a", "s")), Change::set("designs/b", design("b", "s"))], meta()).unwrap();
    assert_eq!(c1.version, 1);
    let c2 = store.apply(vec![Change::delete("designs/a")], meta()).unwrap();
    assert_eq!(c2.version, 2);

    let all = store.read(0, None);
    assert_eq!(all.records.len(), 2, "tombstone kept");
    assert!(all.records["designs/a"].deleted && all.records["designs/a"].value.is_none());
    assert_eq!(all.records["designs/a"].rev, 2);

    let since1 = store.read(1, Some(&c1.epoch));
    assert_eq!(since1.records.keys().collect::<Vec<_>>(), vec!["designs/a"]);
    // Another epoch: everything.
    assert_eq!(store.read(1, Some("other")).records.len(), 2);

    // Serialised like the reference.
    let wire = serde_json::to_value(&all).unwrap();
    assert_eq!(wire["records"]["designs/a"], json!({"rev": 2, "deleted": true}));
    assert_eq!(wire["records"]["designs/b"]["rev"], json!(1));
    assert!(wire["records"]["designs/b"].get("deleted").is_none());
}

#[test]
fn invalid_batch_applies_nothing() {
    let store = Store::in_memory();
    let err = store.apply(vec![Change::set("designs/a", design("a", "s")), Change::set("jobs/x", json!(1))], meta()).unwrap_err();
    assert_eq!(err.to_string(), "Invalid record path (not a shared store key): jobs/x");
    let err = store
        .apply(vec![Change { path: "designs/a".into(), value: None, deleted: false }], meta())
        .unwrap_err();
    assert_eq!(err, ApplyError::MissingValue("designs/a".into()));
    assert_eq!(store.cursor().version, 0);
    assert!(store.snapshot().is_empty());
}

#[test]
fn snapshots_are_immutable_and_typed() {
    let store = Store::in_memory();
    store.apply(vec![Change::set("designs/a", design("a", "s"))], meta()).unwrap();
    let before = store.snapshot();
    let mut d = (*before.design("a").unwrap()).clone();
    d.title = "Renamed".into();
    store.apply(vec![Change::put(RecordKind::Designs, "a", &d).unwrap()], meta()).unwrap();
    assert_eq!(before.design("a").unwrap().title, "T a");
    assert_eq!(store.snapshot().design("a").unwrap().title, "Renamed");
    assert_eq!(store.snapshot().designs().count(), 1);
}

#[test]
fn lossless_roundtrip_and_invalid_records() {
    let mut d = design("a", "s");
    d["futureField"] = json!({"nested": [1, 2]});
    d["request"] = json!({"instruction": "x", "count": 2, "bases": [{"id": "b", "title": "B", "html": ""}]});
    let file: StateFile = serde_json::from_value(json!({
        "epoch": "e1", "version": 7,
        "records": {
            "designs/a": {"rev": 3, "value": d},
            "designs/broken": {"rev": 4, "value": {"id": "broken"}},
            "spaces/s": {"rev": 1, "value": {"id": "s", "name": "S", "parentSpaceId": "root", "createdAt": 5, "kind": "project", "localPath": "/x"}},
            "reviewedIds/a": {"rev": 2, "value": true},
            "jobs/stray": {"rev": 2, "value": 1}
        }
    }))
    .unwrap();
    let store = Store::from_state(file);
    let snap = store.snapshot();
    assert!(snap.get("jobs/stray").is_none(), "non-shared keys are pruned on load");
    // Raw value unchanged.
    assert_eq!(*snap.value("designs/a").unwrap().as_ref(), d);
    // Typed view keeps unknown fields and writes them back.
    let typed = snap.design("a").unwrap();
    assert_eq!(typed.extra["futureField"], json!({"nested": [1, 2]}));
    assert_eq!(typed.request.as_ref().unwrap().extra["bases"][0]["id"], json!("b"));
    assert_eq!(serde_json::to_value(&*typed).unwrap(), d);
    // A record that doesn't fit its type stays (raw) and is served, but has no typed view.
    assert!(snap.design("broken").is_none());
    assert!(matches!(snap.get("designs/broken").unwrap().typed(), Typed::Invalid));
    assert_eq!(snap.read(0, None).records.len(), 4);
    assert!(snap.space("s").unwrap().is_project());
    assert!(snap.is_reviewed("a"));
}

#[test]
fn subscribe_gets_one_event_per_batch() {
    let store = Store::in_memory();
    let mut rx = store.subscribe();
    store.apply(vec![], meta()).unwrap();
    store.apply(vec![Change::set("portals/p", json!({"x": 1, "y": 2})), Change::set("lanes/j", json!(1))], meta()).unwrap();
    let ev = rx.try_recv().unwrap();
    assert_eq!(ev.version, 1);
    assert_eq!(ev.client_id.as_deref(), Some("test"));
    assert_eq!(&*ev.paths, &["portals/p".to_string(), "lanes/j".to_string()]);
    assert_eq!(serde_json::to_value(&ev).unwrap(), json!({"epoch": ev.epoch, "version": 1, "clientId": "test"}));
    assert!(rx.try_recv().is_err(), "the empty batch sent nothing");
    assert_eq!(store.snapshot().portal("p"), Some(Point { x: 1.0, y: 2.0 }));
}

#[test]
fn persists_atomically_with_log() {
    let tmp = Tmp::new();
    let epoch = {
        let store = Store::open(&tmp.0).unwrap();
        store.apply(vec![Change::set("designs/a", design("a", "s"))], meta()).unwrap();
        store.apply(vec![Change::set("designs/a", design("a", "s2")), Change::delete("designs/a")], meta()).unwrap();
        store.apply(vec![Change::set("spaces/s", json!({"id": "s", "name": "Space", "createdAt": 1}))], ChangeMeta::default()).unwrap();
        store.flush().unwrap();
        store.cursor().epoch
    };
    // No tmp files left behind.
    let names: Vec<String> = std::fs::read_dir(&tmp.0).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");

    let reopened = Store::open(&tmp.0).unwrap();
    let c = reopened.cursor();
    assert_eq!((c.epoch.as_str(), c.version), (epoch.as_str(), 3));
    let snap = reopened.snapshot();
    assert!(snap.get("designs/a").unwrap().is_deleted());
    assert_eq!(snap.space("s").unwrap().name, "Space");

    let log = std::fs::read_to_string(tmp.0.join(LOG_FILE)).unwrap();
    let lines: Vec<Value> = log.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0]["op"], "create");
    assert_eq!(lines[0]["before"], "absent");
    assert_eq!(lines[0]["title"], "T a");
    assert_eq!(lines[0]["clientId"], "test");
    assert_eq!(lines[0]["reason"], "unit");
    assert_eq!(lines[1]["op"], "update");
    assert_eq!(lines[1]["prevRev"], 1);
    assert_eq!(lines[2]["op"], "delete");
    assert_eq!(lines[2]["title"], "T a");
    assert_eq!(lines[3]["title"], "Space");
    assert!(lines[3].get("clientId").is_none());
    let ts = lines[0]["ts"].as_str().unwrap();
    assert!(ts.len() == 24 && ts.ends_with('Z') && ts.as_bytes()[10] == b'T', "{ts}");
}

#[test]
fn saves_after_drop_without_flush() {
    let tmp = Tmp::new();
    {
        let store = Store::open(&tmp.0).unwrap();
        store.apply(vec![Change::set("designs/a", design("a", "s"))], meta()).unwrap();
    }
    // The writer thread does a final save when the last handle is dropped; give it a moment.
    let path = tmp.0.join(STATE_FILE);
    for _ in 0..100 {
        if path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let file: StateFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(file.version, 1);
}

#[test]
fn import_reference_copies_once_and_never_writes_the_source() {
    let src_dir = Tmp::new();
    let src = src_dir.0.join(STATE_FILE);
    let body = serde_json::to_vec(&json!({"epoch": "ref", "version": 2, "records": {"designs/a": {"rev": 2, "value": design("a", "s")}}})).unwrap();
    std::fs::write(&src, &body).unwrap();
    let data = Tmp::new();
    let dir = data.0.join("nested/dir");

    assert_eq!(import_reference(&src, &dir).unwrap(), ImportOutcome::Imported { records: 1 });
    assert_eq!(import_reference(&src, &dir).unwrap(), ImportOutcome::AlreadyPresent);
    assert_eq!(std::fs::read(&src).unwrap(), body, "source untouched");

    // Another test spawns a child process; for an instant that child can still hold a copy of our released lock.
    let store = (0..100)
        .find_map(|_| match Store::open(&dir) {
            Ok(s) => Some(s),
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(20));
                None
            }
        })
        .expect("opening the imported data dir");
    assert_eq!(store.cursor().epoch, "ref");
    store.apply(vec![Change::delete("designs/a")], meta()).unwrap();
    store.flush().unwrap();
    assert_eq!(std::fs::read(&src).unwrap(), body, "writes go to the data dir only");
    assert_eq!(import_reference(&src, &dir).unwrap(), ImportOutcome::AlreadyPresent, "never overwrites");

    // A source that isn't a state file is rejected and leaves nothing behind.
    std::fs::write(&src, b"{not json").unwrap();
    let other = data.0.join("other");
    assert!(import_reference(&src, &other).is_err());
    assert!(!other.join(STATE_FILE).exists());
}

#[test]
fn data_dir_env_override() {
    // Only checks the default shape (the env var is process-global, so don't set it in parallel tests).
    let d = default_data_dir();
    if std::env::var_os("PROTOTYPIE_DATA").is_none() {
        assert!(d.ends_with("Library/Application Support/Prototypie"), "{}", d.display());
    }
}

/// Read-only round trip of a real web-app state, if `PROTOTYPIE_REFERENCE_DATA` names one: `cargo test -p proto-core -- --ignored`.
#[test]
#[ignore]
fn reference_state_roundtrip() {
    let Some(src) = reference_state_path() else { return };
    let Ok(bytes) = std::fs::read(&src) else { return };
    let file: StateFile = serde_json::from_slice(&bytes).unwrap();
    let n = file.records.len();
    let store = Store::from_state(file.clone());
    let snap = store.snapshot();
    assert_eq!(snap.read(0, None), file, "lossless");
    let invalid: Vec<&str> = file
        .records
        .keys()
        .filter(|p| matches!(snap.get(p).map(|e| (e.is_deleted(), e.typed())), Some((false, Typed::Invalid))))
        .map(String::as_str)
        .collect();
    assert!(invalid.is_empty(), "records that don't fit their type: {invalid:?}");
    // Typed views write every field back, except explicit nulls on optional fields (written as absent; the
    // reference treats both the same). Raw values, which the server serves, keep them.
    fn strip_nulls(v: &Value) -> Value {
        match v {
            Value::Object(m) => Value::Object(m.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), strip_nulls(v))).collect()),
            Value::Array(a) => Value::Array(a.iter().map(strip_nulls).collect()),
            v => v.clone(),
        }
    }
    let mut mismatched = Vec::new();
    for kind in RecordKind::ALL {
        for (id, e) in snap.live(kind) {
            let typed = match e.typed() {
                Typed::Design(x) => serde_json::to_value(&**x),
                Typed::Space(x) => serde_json::to_value(&**x),
                Typed::Run(x) => serde_json::to_value(&**x),
                Typed::Portal(x) => serde_json::to_value(x),
                Typed::PromotedFrom(x) => serde_json::to_value(&**x),
                Typed::Notes(x) => serde_json::to_value(&**x),
                Typed::Lane(x) => serde_json::to_value(&**x),
                Typed::PublishedPoll(x) => serde_json::to_value(&**x),
                _ => continue,
            }
            .unwrap();
            if typed != strip_nulls(e.value.as_ref().unwrap()) {
                mismatched.push(format!("{}/{id}", kind.as_str()));
            }
        }
    }
    assert!(mismatched.is_empty(), "{} typed records don't round-trip: {:?}", mismatched.len(), &mismatched[..mismatched.len().min(10)]);
    assert!(!snap.projects().is_empty());
    eprintln!("{n} records, {} designs, {} projects", snap.designs().count(), snap.projects().len());
}

#[test]
fn import_copies_reference_assets_once() {
    // <repo>/data/state.json + <repo>/public/{vendor,imports,other}
    let repo = Tmp::new();
    std::fs::create_dir_all(repo.0.join("data")).unwrap();
    std::fs::write(repo.0.join("data").join(STATE_FILE), br#"{"epoch":"r","version":0,"records":{}}"#).unwrap();
    let vendor = repo.0.join("public/vendor/usage-direct");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(vendor.join("styles.css"), b"a{}").unwrap();
    std::fs::create_dir_all(repo.0.join("public/imports")).unwrap();
    std::fs::write(repo.0.join("public/imports/bundle.json"), b"{}").unwrap();
    std::fs::create_dir_all(repo.0.join("public/other")).unwrap();
    std::fs::write(repo.0.join("public/other/x.txt"), b"x").unwrap();

    let data = Tmp::new();
    assert_eq!(import_reference(&repo.0.join("data").join(STATE_FILE), &data.0).unwrap(), ImportOutcome::Imported { records: 0 });
    let public = public_dir(&data.0);
    assert_eq!(std::fs::read(public.join("vendor/usage-direct/styles.css")).unwrap(), b"a{}");
    assert!(public.join("imports/bundle.json").exists());
    assert!(!public.join("other").exists(), "only vendor/ and imports/");

    // A local edit survives a second import; a new reference file is added.
    std::fs::write(public.join("vendor/usage-direct/styles.css"), b"edited").unwrap();
    std::fs::write(vendor.join("new.js"), b"1").unwrap();
    let again = import_reference_assets(&repo.0.join("public"), &data.0).unwrap();
    assert_eq!(again, AssetImport { copied: 1, kept: 2 });
    assert_eq!(std::fs::read(public.join("vendor/usage-direct/styles.css")).unwrap(), b"edited");
    assert_eq!(std::fs::read(vendor.join("styles.css")).unwrap(), b"a{}", "source untouched");
    let leftovers: Vec<_> = std::fs::read_dir(public.join("vendor/usage-direct")).unwrap().map(|e| e.unwrap().file_name()).filter(|n| n.to_string_lossy().ends_with(".tmp")).collect();
    assert!(leftovers.is_empty());
}

#[test]
fn one_writer_per_data_dir() {
    let tmp = Tmp::new();
    assert_eq!(lock_holder(&tmp.0), None, "no lock file yet");
    let store = Store::open(&tmp.0).unwrap();
    store.apply(vec![Change::set("designs/a", design("a", "s"))], meta()).unwrap();
    let me = std::process::id();
    assert_eq!(lock_holder(&tmp.0), Some(me));
    // A second writer (same process or another) is refused with the holder's pid.
    let err = Store::open(&tmp.0).unwrap_err();
    assert!(matches!(err.downcast_ref::<StoreError>(), Some(StoreError::Locked { pid }) if *pid == me), "{err:#}");
    assert!(err.to_string().contains(&format!("pid {me}")));
    // Read-only users load a copy and need no lock; clones share the one lock.
    let copy = store.clone();
    drop(store);
    assert_eq!(lock_holder(&tmp.0), Some(me), "a clone still holds it");
    // Dropping the last handle saves and releases before returning.
    drop(copy);
    assert_eq!(lock_holder(&tmp.0), None);
    let again = Store::open(&tmp.0).unwrap();
    assert_eq!(again.snapshot().version, 1, "the final save landed before the lock was released");
    // import_reference with the dir in use: a no-op when a state exists.
    let src = Tmp::new();
    std::fs::write(src.0.join(STATE_FILE), br#"{"epoch":"r","version":0,"records":{}}"#).unwrap();
    assert_eq!(import_reference(&src.0.join(STATE_FILE), &tmp.0).unwrap(), ImportOutcome::AlreadyPresent);
}

/// Helper for `lock_released_when_the_holder_process_dies`: holds a store until killed.
#[test]
#[ignore]
fn lock_child_holder() {
    let Some(dir) = std::env::var_os("PROTO_CORE_LOCK_CHILD") else { return };
    let _store = Store::open(PathBuf::from(dir)).unwrap();
    println!("LOCKED");
    std::thread::sleep(std::time::Duration::from_secs(60));
}

#[test]
fn lock_released_when_the_holder_process_dies() {
    use std::io::BufRead;
    let tmp = Tmp::new();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lock_child_holder", "--ignored", "--nocapture", "--test-threads", "1"])
        .env("PROTO_CORE_LOCK_CHILD", &tmp.0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let out = std::io::BufReader::new(child.stdout.take().unwrap());
    assert!(out.lines().map_while(Result::ok).any(|l| l.contains("LOCKED")), "child never locked");
    assert_eq!(lock_holder(&tmp.0), Some(child.id()));
    let err = Store::open(&tmp.0).unwrap_err();
    assert!(matches!(err.downcast_ref::<StoreError>(), Some(StoreError::Locked { pid }) if *pid == child.id()), "{err:#}");
    // SIGKILL: no destructors run; the kernel still drops the lock.
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(lock_holder(&tmp.0), None);
    Store::open(&tmp.0).unwrap();
}
