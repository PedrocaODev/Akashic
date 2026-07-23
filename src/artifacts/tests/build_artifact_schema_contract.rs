use crate::artifacts;
use artifacts::{canonical_v2_connection, Store};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

fn snapshot(db: &Connection) -> String {
    let mut out = String::new();
    let mut objects = db
        .prepare("SELECT type,name,coalesce(sql,'') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name")
        .unwrap();
    for row in objects
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap()
    {
        let (kind, name, sql) = row.unwrap();
        out.push_str(&format!(
            "{kind}:{name}:{}\n",
            sql.split_whitespace().collect::<Vec<_>>().join(" ")
        ));
        if kind == "table" {
            let mut columns = db.prepare(&format!("PRAGMA table_info({name})")).unwrap();
            for c in columns
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, i64>(5)?,
                    ))
                })
                .unwrap()
            {
                out.push_str(&format!("column:{name}:{:?}\n", c.unwrap()));
            }
            let mut indexes = db.prepare(&format!("PRAGMA index_list({name})")).unwrap();
            for i in indexes.query_map([], |r| r.get::<_, String>(1)).unwrap() {
                let index = i.unwrap();
                out.push_str(&format!("index:{name}:{index}:"));
                let mut info = db.prepare(&format!("PRAGMA index_info({index})")).unwrap();
                for column in info
                    .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(2)?)))
                    .unwrap()
                {
                    out.push_str(&format!("{:?};", column.unwrap()));
                }
                out.push('\n');
            }
            let mut foreign = db
                .prepare(&format!("PRAGMA foreign_key_list({name})"))
                .unwrap();
            for f in foreign
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                })
                .unwrap()
            {
                out.push_str(&format!("foreign:{name}:{:?}\n", f.unwrap()));
            }
        }
    }
    out.push_str(&format!(
        "metadata:{:?}\n",
        db.prepare("SELECT key,value FROM schema_metadata ORDER BY key")
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect::<Vec<_>>()
    ));
    out
}

#[test]
fn canonical_v2_and_v3_physical_schema_contracts_are_stable() {
    let v2 = canonical_v2_connection().unwrap();
    let path =
        std::env::temp_dir().join(format!("akashic-schema-contract-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let v3 = Store::open(&path).unwrap();
    let v3_snapshot = snapshot(v3.connection());
    drop(v3);
    let v3_reopened = Connection::open(&path).unwrap();
    let v2_snapshot = snapshot(&v2);
    assert!(!v2_snapshot.is_empty());
    assert_eq!(v3_snapshot, snapshot(&v3_reopened));
    assert!(!v3_snapshot.contains("recovery_ledger"));
    let hash = |value: &str| format!("{:x}", Sha256::digest(value.as_bytes()));
    assert_eq!(
        hash(&v2_snapshot),
        "544a58567de00af9569635bb9d59f3294c06c264f483070f5bbc326ca4e16a80"
    );
    assert_eq!(
        hash(&v3_snapshot),
        "f307e8760a5309c8fd396949f092b0b321132d252dfde94bfbdbdf62441dbf6a"
    );
    drop(v3_reopened);
    std::fs::remove_file(path).unwrap();
}
