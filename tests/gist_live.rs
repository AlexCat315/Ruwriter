//! Live end-to-end check against a real private Gist.
//!
//! Run with:
//! `RUWRITER_GIST_TOKEN=$(gh auth token) cargo test --test gist_live -- --ignored --nocapture`
//!
//! The test creates a private Gist, syncs two sessions, and deletes the Gist
//! on the way out. It never prints the token.

use std::env;

use ruwriter::gist::{SyncRequest, delete_gist, sync_with_gist};
use ruwriter_core::Session;

struct Cleanup {
    token: String,
    id: String,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = delete_gist(&self.token, &self.id);
    }
}

#[test]
#[ignore = "creates and deletes a real private Gist"]
fn live_two_device_round_trip() {
    let token = env::var("RUWRITER_GIST_TOKEN").expect("RUWRITER_GIST_TOKEN");
    let device = "ruwriter-test".to_owned();

    let first = tempfile::tempdir().unwrap();
    let mut a = Session::at(first.path());
    a.save("第一章 雨夜\n\n雨落在旧城的屋檐上。").unwrap();
    let library_id = a.sync_config().library_id.clone();
    let local = a.sync_snapshot();
    let report = sync_with_gist(&SyncRequest {
        token: token.clone(),
        gist_id: None,
        library_id: library_id.clone(),
        device: device.clone(),
        local,
        base: Default::default(),
    })
    .unwrap();
    assert!(!report.gist_id.is_empty());
    let cleanup = Cleanup {
        token: token.clone(),
        id: report.gist_id.clone(),
    };

    // A second pass with the base from the first pass must not upload again.
    let local = a.sync_snapshot();
    let second = sync_with_gist(&SyncRequest {
        token: token.clone(),
        gist_id: Some(report.gist_id.clone()),
        library_id: library_id.clone(),
        device: device.clone(),
        local,
        base: report.base.clone(),
    })
    .unwrap();
    assert!(second.pulled.is_empty());
    assert!(second.deleted.is_empty());
    assert_eq!(second.gist_id, cleanup.id);

    // A fresh device joins the same Gist and imports the chapter.
    let second_dir = tempfile::tempdir().unwrap();
    let mut b = Session::at(second_dir.path());
    let mut config = b.sync_config().clone();
    config.library_id = library_id.clone();
    b.set_sync_config(config);
    let pulled = sync_with_gist(&SyncRequest {
        token: token.clone(),
        gist_id: None,
        library_id: library_id.clone(),
        device: "ruwriter-test-2".to_owned(),
        local: Vec::new(),
        base: Default::default(),
    })
    .unwrap();
    assert!(
        pulled
            .pulled
            .iter()
            .any(|d| d.content.contains("雨落在旧城")),
        "the remote chapter should be offered to the new device"
    );
    b.apply_sync_report(&pulled).unwrap();
    assert!(
        b.documents().iter().any(|d| d.title.contains("雨夜")),
        "the chapter should be imported"
    );

    // A local edit after the join is uploaded, not treated as a conflict.
    a.save("第一章 雨夜\n\n雨落在旧城的屋檐上。风更大了。")
        .unwrap();
    let local = a.sync_snapshot();
    let edited = sync_with_gist(&SyncRequest {
        token,
        gist_id: Some(cleanup.id.clone()),
        library_id,
        device,
        local,
        base: report.base,
    })
    .unwrap();
    assert!(!edited.manifest.documents.is_empty());
    assert_eq!(edited.gist_id, cleanup.id);
}
