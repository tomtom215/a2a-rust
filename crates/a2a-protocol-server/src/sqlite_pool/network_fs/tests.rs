// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use super::*;

/// A `/proc/self/mountinfo` with every case the lookup has to tell apart.
const MOUNTINFO: &str = "\
22 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw
30 22 0:27 / /data rw,relatime shared:5 - nfs4 fileserver:/export rw,vers=4.2
31 30 0:28 / /data/local rw shared:6 - tmpfs tmpfs rw
32 22 0:29 / /mnt/with\\040space rw - cifs //srv/share rw
33 22 0:30 / /mnt/fuse rw - fuse.sshfs me@host:/ rw
34 22 0:31 / /mnt/ntfs rw - fuseblk /dev/sdb1 rw
35 22 0:32 / /mnt/over rw - nfs server:/a rw
36 22 0:33 / /mnt/over rw - ext4 /dev/sdc rw
";

fn fs(path: &str) -> Option<String> {
    filesystem_of(MOUNTINFO, Path::new(path))
}

#[test]
fn the_mount_with_the_longest_containing_mount_point_holds_the_file() {
    assert_eq!(
        fs("/home/agent/a2a.db").as_deref(),
        Some("ext4"),
        "the root"
    );
    assert_eq!(fs("/data/a2a.db").as_deref(), Some("nfs4"));
    assert_eq!(
        fs("/data/local/a2a.db").as_deref(),
        Some("tmpfs"),
        "a mount inside a mount holds what is under it"
    );
}

/// `/database` is not under `/data`: containment is by path component, not by
/// string prefix.
#[test]
fn a_sibling_that_shares_a_prefix_is_not_inside_the_mount() {
    assert_eq!(fs("/database/a2a.db").as_deref(), Some("ext4"));
}

#[test]
fn an_escaped_mount_point_is_read_as_the_path_it_names() {
    assert_eq!(fs("/mnt/with space/a2a.db").as_deref(), Some("cifs"));
}

/// A mount placed over another at the same mount point is the one in force.
#[test]
fn the_later_of_two_mounts_at_one_mount_point_wins() {
    assert_eq!(fs("/mnt/over/a2a.db").as_deref(), Some("ext4"));
}

#[test]
fn lines_that_are_not_mounts_are_skipped_and_nothing_found_is_none() {
    let mountinfo = "garbage with no separator\n1 2 3 - ext4\n\n9 1 0:1 / /x rw - nfs s rw\n";
    assert_eq!(
        filesystem_of(mountinfo, Path::new("/x/db")).as_deref(),
        Some("nfs"),
        "the one well-formed line is used"
    );
    assert_eq!(filesystem_of(mountinfo, Path::new("/elsewhere/db")), None);
    assert_eq!(filesystem_of("", Path::new("/x")), None);
}

#[test]
fn network_and_fuse_file_systems_are_remote_and_local_ones_are_not() {
    for remote in [
        "nfs",
        "nfs4",
        "cifs",
        "smb3",
        "smbfs",
        "9p",
        "ceph",
        "glusterfs",
        "lustre",
        "afs",
        "ncpfs",
        "beegfs",
        "gpfs",
        "orangefs",
        "panfs",
        "fuse",
        "fuse.sshfs",
        "fuse.s3fs",
    ] {
        assert!(is_remote(remote), "{remote} is remote");
    }
    for local in [
        "ext4", "xfs", "btrfs", "zfs", "tmpfs", "overlay", "vfat", "fuseblk", "fusectl", "ntfs3",
        "",
    ] {
        assert!(!is_remote(local), "{local:?} is local");
    }
}

#[test]
fn only_a_remote_mount_is_reported() {
    let remote = |p: &str| remote_filesystem(MOUNTINFO, Path::new(p));
    assert_eq!(remote("/data/a2a.db").as_deref(), Some("nfs4"));
    assert_eq!(remote("/mnt/fuse/a2a.db").as_deref(), Some("fuse.sshfs"));
    assert_eq!(remote("/mnt/ntfs/a2a.db"), None, "fuseblk is a local disk");
    assert_eq!(remote("/data/local/a2a.db"), None);
    assert_eq!(remote("/home/a2a.db"), None);
}

#[test]
fn octal_escapes_are_decoded_and_anything_else_is_left_alone() {
    assert_eq!(unescape("/plain"), "/plain");
    assert_eq!(unescape("/a\\040b"), "/a b");
    assert_eq!(unescape("/a\\011b\\012c"), "/a\tb\nc");
    assert_eq!(unescape("/a\\134b"), "/a\\b");
    assert_eq!(unescape("/two\\040\\040spaces"), "/two  spaces");
    assert_eq!(unescape("/a\\9zb"), "/a\\9zb", "not octal: kept as written");
    assert_eq!(unescape("/a\\400b"), "/a\\400b", "256 is not a byte: kept");
    assert_eq!(unescape("/a\\04"), "/a\\04", "too short: kept");
    // The kernel escapes only space, tab, newline and backslash, so a byte
    // above 0x7f is never an escape, whatever its digits say.
    assert_eq!(unescape("/a\\303b"), "/a\\303b", "not ASCII: kept");
    assert_eq!(unescape("/end\\"), "/end\\");
}

#[test]
fn the_file_a_database_names_is_resolved_through_its_directory() {
    let dir = std::env::temp_dir().canonicalize().expect("temp dir");
    assert_eq!(
        resolved(&dir.join("a2a.db")),
        Some(dir.join("a2a.db")),
        "a file that does not exist yet resolves through its directory"
    );
    assert_eq!(
        resolved(Path::new("relative-a2a.db")),
        Some(
            std::env::current_dir()
                .and_then(|d| d.canonicalize())
                .expect("cwd")
                .join("relative-a2a.db")
        ),
        "a bare name is in the working directory"
    );
    assert_eq!(resolved(Path::new(":memory:")), None);
    assert_eq!(resolved(Path::new("")), None);
    // What `sqlx` hands back for `sqlite::memory:`, and a URI of one's own: not
    // files in the working directory, so not resolved as if they were.
    assert_eq!(resolved(Path::new("file:sqlx-in-memory-0")), None);
    assert_eq!(resolved(Path::new("file:/data/a2a.db?mode=rwc")), None);
    assert_eq!(
        resolved(Path::new("filename-that-merely-starts-with-file.db")),
        Some(
            std::env::current_dir()
                .and_then(|d| d.canonicalize())
                .expect("cwd")
                .join("filename-that-merely-starts-with-file.db")
        ),
        "only the `file:` scheme is a URI"
    );
    assert_eq!(
        resolved(&dir.join("no-such-directory-e-f26").join("a2a.db")),
        None,
        "a directory that cannot be resolved is not guessed at"
    );
}

/// On Linux the real table is read and a file under the temp directory is on
/// some mount; whether that mount is local is the machine's business.
#[cfg(target_os = "linux")]
#[test]
fn the_real_mount_table_names_a_file_system_for_the_temp_directory() {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").expect("mountinfo");
    let dir = std::env::temp_dir().canonicalize().expect("temp dir");
    assert!(filesystem_of(&mountinfo, &dir.join("a2a.db")).is_some());
}

#[cfg(feature = "tracing")]
mod warnings {
    use super::*;
    use std::sync::Arc;
    use tracing_subscriber::layer::SubscriberExt as _;

    /// The text of every `WARN` event, in order.
    #[derive(Clone, Default)]
    struct Warnings(Arc<Mutex<Vec<String>>>);

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Warnings {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            use std::fmt::Write as _;
            struct Text(String);
            impl tracing::field::Visit for Text {
                fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                    let _ = write!(self.0, "{}={v:?} ", f.name());
                }
            }
            if *event.metadata().level() == tracing::Level::WARN {
                let mut text = Text(String::new());
                event.record(&mut text);
                self.0.lock().expect("lock").push(text.0);
            }
        }
    }

    /// One warning per file, naming the file and the file system, and none
    /// for the second ask.
    #[test]
    fn a_remote_file_is_warned_about_once() {
        let warnings = Warnings::default();
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(warnings.clone()));
        let seen = Mutex::default();
        let path = Path::new("/data/a2a.db");

        assert!(warn_once(&seen, path, "nfs4"));
        assert!(!warn_once(&seen, path, "nfs4"), "the second is silent");
        assert!(warn_once(&seen, Path::new("/data/other.db"), "nfs4"));

        let said = warnings.0.lock().expect("lock").clone();
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].contains("/data/a2a.db"), "{}", said[0]);
        assert!(said[0].contains("nfs4"), "{}", said[0]);
        assert!(said[0].contains("network or FUSE"), "{}", said[0]);
    }

    /// A mount table that puts the temp directory on `fstype`, written to a
    /// file of its own.
    fn mount_table(fstype: &str, tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().canonicalize().expect("temp dir");
        let table =
            std::env::temp_dir().join(format!("a2a-e-f26-mountinfo-{tag}-{}", std::process::id()));
        // The working directory is on the same mount, so a database that is
        // wrongly read as a path in it (an in-memory one) is warned about
        // wherever the test runs: under cargo-mutants the tree is copied under
        // the temp directory, and without this the bug showed only there.
        let cwd = std::env::current_dir()
            .and_then(|d| d.canonicalize())
            .expect("working directory");
        std::fs::write(
            &table,
            format!(
                "22 1 8:1 / / rw - ext4 /dev/sda1 rw\n30 22 0:27 / {} rw - {fstype} srv:/x rw\n31 22 0:28 / {} rw - {fstype} srv:/y rw\n",
                dir.display(),
                cwd.display()
            ),
        )
        .expect("write the mount table");
        table
    }

    /// Opens the pool for `name` in the temp directory against `table` and
    /// removes what it created.
    async fn open(name: &str, table: &Path) {
        let path = std::env::temp_dir().join(name);
        let pool = crate::sqlite_pool::sqlite_pool_with_size(
            &format!("sqlite://{}", path.display()),
            1,
            table,
        )
        .await
        .expect("pool");
        pool.close().await;
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    /// Opening a database that the mount table puts on NFS warns, naming the
    /// file, once however many stores open it; one on a local mount, and an
    /// in-memory one, say nothing.
    #[tokio::test]
    async fn opening_a_database_on_a_network_mount_warns_once() {
        let warnings = Warnings::default();
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(warnings.clone()));
        let tag = std::process::id();
        let on_nfs = format!("a2a-e-f26-nfs-{tag}.db");
        let nfs = mount_table("nfs4", "nfs");

        open(&on_nfs, &nfs).await;
        open(&on_nfs, &nfs).await;
        open(&format!("a2a-e-f26-nfs-other-{tag}.db"), &nfs).await;
        open(
            &format!("a2a-e-f26-local-{tag}.db"),
            &mount_table("ext4", "local"),
        )
        .await;
        let memory = crate::sqlite_pool::sqlite_pool_with_size("sqlite::memory:", 1, &nfs)
            .await
            .expect("pool");
        memory.close().await;
        // A table that cannot be read says nothing either.
        open(
            &format!("a2a-e-f26-nomount-{tag}.db"),
            Path::new("/no/such/mountinfo"),
        )
        .await;

        let said = warnings.0.lock().expect("lock").clone();
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].contains(&on_nfs), "{}", said[0]);
        assert!(said[0].contains("nfs4"), "{}", said[0]);
        assert!(said[1].contains("a2a-e-f26-nfs-other"), "{}", said[1]);
        let _ = std::fs::remove_file(nfs);
        let _ = std::fs::remove_file(mount_table("ext4", "local"));
    }
}
