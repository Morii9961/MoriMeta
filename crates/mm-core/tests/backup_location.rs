// SPDX-License-Identifier: GPL-3.0-or-later
//! A backup location on a share is refused by its form, before anything opens it: opening a
//! share connects to the server and Windows sends the user's credentials (SECURITY_MODEL §6).

use mm_core::executor::check_backup_location;
use mm_store::Store;

#[test]
fn a_share_is_refused_without_being_touched() {
    let d = std::env::temp_dir().join(format!("mm-backup-share-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let mut store = Store::open(&d).unwrap();
    // a host that cannot exist: had it been touched, the error would be a lookup or IO error
    for root in [
        r"\\mm-no-such-host.invalid\share\backups",
        "//mm-no-such-host.invalid/share/backups",
    ] {
        store.set_backup_root(root.into());
        let started = std::time::Instant::now();
        let err = check_backup_location(&store).unwrap_err().to_string();
        assert!(err.contains("must be on a local disk"), "{root}: {err}");
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "{root} was looked up"
        );
    }
    // a local folder still passes
    store.set_backup_root(d.join("backups"));
    check_backup_location(&store).unwrap();
    let _ = std::fs::remove_dir_all(&d);
}
