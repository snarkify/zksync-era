//! Service tasks for the Merkle tree.

use std::{
    ops,
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use rayon::prelude::*;

use crate::{
    types::{NodeKey, StaleNodeKey},
    Database, PruneDatabase, RocksDBWrapper,
};

#[derive(Debug)]
pub(crate) struct StaleKeysRepairData {
    pub next_version: u64,
}

/// Handle for a [`StaleKeysRepairTask`] allowing to abort its operation.
///
/// The task is aborted once the handle is dropped.
#[must_use = "Paired `StaleKeysRepairTask` is aborted once handle is dropped"]
#[derive(Debug)]
pub struct StaleKeysRepairHandle {
    _aborted_sender: mpsc::Sender<()>,
}

/// Task that repairs stale keys for the tree.
///
/// Early tree versions contained a bug: If a tree version was truncated, stale keys for it remained intact.
/// If an overwritten tree version did not contain the same keys, this led to keys incorrectly marked as stale,
/// meaning that after pruning, a tree may end up broken.
#[derive(Debug)]
pub struct StaleKeysRepairTask {
    db: RocksDBWrapper,
    parallelism: u64,
    poll_interval: Duration,
    aborted_receiver: mpsc::Receiver<()>,
}

impl StaleKeysRepairTask {
    /// Creates a new task.
    pub fn new(db: RocksDBWrapper) -> (Self, StaleKeysRepairHandle) {
        let (aborted_sender, aborted_receiver) = mpsc::channel();
        let this = Self {
            db,
            parallelism: (rayon::current_num_threads() as u64).max(1),
            poll_interval: Duration::from_secs(60),
            aborted_receiver,
        };
        let handle = StaleKeysRepairHandle {
            _aborted_sender: aborted_sender,
        };
        (this, handle)
    }

    /// Runs stale key detection for a single tree version.
    #[tracing::instrument(skip(db))]
    pub fn run_for_version(db: &RocksDBWrapper, version: u64) -> anyhow::Result<Vec<NodeKey>> {
        const SAMPLE_COUNT: usize = 5;

        let version_keys = db
            .all_keys_for_version(version)
            .with_context(|| format!("failed loading keys changed in tree version {version}"))?;
        let stale_keys = db.stale_keys(version);

        if !version_keys.unreachable_keys.is_empty() {
            let keys_sample: Vec<_> = version_keys
                .unreachable_keys
                .iter()
                .take(SAMPLE_COUNT)
                .collect::<Vec<_>>();
            tracing::warn!(
                version,
                unreachable_keys.len = version_keys.unreachable_keys.len(),
                unreachable_keys.sample = ?keys_sample,
                "Found unreachable keys in tree"
            );
        }

        let mut bogus_stale_keys = vec![];
        for stale_key in stale_keys {
            if version_keys.valid_keys.contains(&stale_key.nibbles) {
                // Normal case: a new node obsoletes a previous version.
            } else if version_keys.unreachable_keys.contains(&stale_key.nibbles) {
                // Explainable bogus stale key: a node that was updated in `version` before the truncation is no longer updated after truncation.
                bogus_stale_keys.push(stale_key);
            } else {
                tracing::warn!(
                    version,
                    ?stale_key,
                    "Unexplained bogus stale key: not present in any nodes changed in the tree version"
                );
                bogus_stale_keys.push(stale_key);
            }
        }

        if bogus_stale_keys.is_empty() {
            return Ok(vec![]);
        }

        let keys_sample: Vec<_> = bogus_stale_keys.iter().take(SAMPLE_COUNT).collect();
        tracing::info!(
            stale_keys.len = bogus_stale_keys.len(),
            stale_keys.sample = ?keys_sample,
            "Found bogus stale keys"
        );

        Ok(bogus_stale_keys)
    }

    /// Returns a boolean flag indicating whether the task data was updated.
    fn step(&mut self) -> anyhow::Result<bool> {
        let repair_data = self
            .db
            .stale_keys_repair_data()
            .context("failed getting repair data")?;
        let min_stale_key_version = self.db.min_stale_key_version();
        let start_version = match (repair_data, min_stale_key_version) {
            (_, None) => {
                tracing::debug!("No stale keys in tree, nothing to do");
                return Ok(false);
            }
            (None, Some(version)) => version,
            (Some(data), Some(version)) => data.next_version.max(version),
        };

        let latest_version = self
            .db
            .manifest()
            .and_then(|manifest| manifest.version_count.checked_sub(1));
        let Some(latest_version) = latest_version else {
            tracing::warn!(
                min_stale_key_version,
                "Tree has stale keys, but no latest versions"
            );
            return Ok(false);
        };

        let end_version = (start_version + self.parallelism - 1).min(latest_version);
        let versions = start_version..=end_version;
        if versions.is_empty() {
            tracing::debug!(?versions, latest_version, "No tree versions to check");
            return Ok(false);
        }

        tracing::debug!(
            ?versions,
            latest_version,
            ?min_stale_key_version,
            "Checking stale keys"
        );

        let stale_keys = versions
            .clone()
            .into_par_iter()
            .map(|version| {
                Self::run_for_version(&self.db, version).map(|output| {
                    output
                        .into_iter()
                        .map(|key| StaleNodeKey::new(key, version))
                        .collect::<Vec<_>>()
                })
            })
            .try_reduce(Vec::new, |mut acc, keys| {
                acc.extend(keys);
                Ok(acc)
            })?;
        self.update_task_data(versions, &stale_keys)?;
        Ok(true)
    }

    #[tracing::instrument(
        level = "debug",
        err,
        skip(self, removed_keys),
        fields(removed_keys.len = removed_keys.len()),
    )]
    fn update_task_data(
        &mut self,
        versions: ops::RangeInclusive<u64>,
        removed_keys: &[StaleNodeKey],
    ) -> anyhow::Result<()> {
        tracing::debug!("Updating task data");
        let started_at = Instant::now();
        let new_data = StaleKeysRepairData {
            next_version: *versions.end() + 1,
        };
        self.db
            .repair_stale_keys(&new_data, removed_keys)
            .context("failed removing bogus stale keys")?;
        let latency = started_at.elapsed();
        tracing::debug!(?latency, "Updated task data");
        Ok(())
    }

    fn wait_for_abort(&mut self, timeout: Duration) -> bool {
        match self.aborted_receiver.recv_timeout(timeout) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => false,
        }
    }

    /// Runs this task indefinitely.
    ///
    /// # Errors
    ///
    /// Propagates RocksDB I/O errors.
    pub fn run(mut self) -> anyhow::Result<()> {
        let mut wait_interval = Duration::ZERO;
        while !self.wait_for_abort(wait_interval) {
            wait_interval = if self.step()? {
                Duration::ZERO
            } else {
                self.poll_interval
            };
        }
        tracing::info!("Stop signal received, stale keys repair is shut down");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;
    use crate::{Key, MerkleTree, MerkleTreePruner, TreeEntry, ValueHash};

    fn setup_tree_with_bogus_stale_keys(db: impl PruneDatabase) {
        let mut tree = MerkleTree::new(db).unwrap();
        let kvs: Vec<_> = (0_u64..100)
            .map(|i| TreeEntry::new(Key::from(i), i + 1, ValueHash::zero()))
            .collect();
        tree.extend(kvs).unwrap();

        let overridden_kvs = vec![TreeEntry::new(
            Key::from(0),
            1,
            ValueHash::repeat_byte(0xaa),
        )];
        tree.extend(overridden_kvs).unwrap();

        let stale_keys = tree.db.stale_keys(1);
        assert!(
            stale_keys.iter().any(|key| !key.is_empty()),
            "{stale_keys:?}"
        );

        // Revert `overridden_kvs`.
        tree.truncate_recent_versions_incorrectly(1).unwrap();
        assert_eq!(tree.latest_version(), Some(0));
        let future_stale_keys = tree.db.stale_keys(1);
        assert!(!future_stale_keys.is_empty());

        // Add a new version without the key. To make the matter more egregious, the inserted key
        // differs from all existing keys, starting from the first nibble.
        let new_key = Key::from_big_endian(&[0xaa; 32]);
        let new_kvs = vec![TreeEntry::new(new_key, 101, ValueHash::repeat_byte(0xaa))];
        tree.extend(new_kvs).unwrap();
        assert_eq!(tree.latest_version(), Some(1));
    }

    #[test]
    fn stale_keys_repair_with_normal_tree() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut db = RocksDBWrapper::new(temp_dir.path()).unwrap();

        // The task should work fine with future tree versions.
        for version in [0, 1, 100] {
            let bogus_stale_keys = StaleKeysRepairTask::run_for_version(&db, version).unwrap();
            assert!(bogus_stale_keys.is_empty());
        }

        let kvs: Vec<_> = (0_u64..100)
            .map(|i| TreeEntry::new(Key::from(i), i + 1, ValueHash::zero()))
            .collect();
        MerkleTree::new(&mut db).unwrap().extend(kvs).unwrap();

        let bogus_stale_keys = StaleKeysRepairTask::run_for_version(&db, 0).unwrap();
        assert!(bogus_stale_keys.is_empty());
    }

    #[test]
    fn detecting_bogus_stale_keys() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut db = RocksDBWrapper::new(temp_dir.path()).unwrap();
        setup_tree_with_bogus_stale_keys(&mut db);

        let bogus_stale_keys = StaleKeysRepairTask::run_for_version(&db, 1).unwrap();
        assert!(!bogus_stale_keys.is_empty());

        let (mut task, _handle) = StaleKeysRepairTask::new(db);
        task.parallelism = 10; // Ensure that all tree versions are checked at once.
                               // Repair the tree.
        assert!(task.step().unwrap());
        // Check that the tree works fine once it's pruned.
        let (mut pruner, _) = MerkleTreePruner::new(&mut task.db);
        pruner.prune_up_to(1).unwrap().expect("tree was not pruned");

        MerkleTree::new(&mut task.db)
            .unwrap()
            .verify_consistency(1, false)
            .unwrap();

        let bogus_stale_keys = StaleKeysRepairTask::run_for_version(&task.db, 1).unwrap();
        assert!(bogus_stale_keys.is_empty());
        MerkleTree::new(&mut task.db)
            .unwrap()
            .verify_consistency(1, false)
            .unwrap();

        assert!(!task.step().unwrap());
    }

    #[test]
    fn full_stale_keys_task_workflow() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut db = RocksDBWrapper::new(temp_dir.path()).unwrap();
        setup_tree_with_bogus_stale_keys(&mut db);

        let (task, handle) = StaleKeysRepairTask::new(db.clone());
        let task_thread = thread::spawn(|| task.run());

        loop {
            if let Some(task_data) = db.stale_keys_repair_data().unwrap() {
                if task_data.next_version == 2 {
                    // All tree versions are processed.
                    break;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }

        assert!(!task_thread.is_finished());
        drop(handle);
        task_thread.join().unwrap().unwrap();

        let bogus_stale_keys = StaleKeysRepairTask::run_for_version(&db, 1).unwrap();
        assert!(bogus_stale_keys.is_empty());
    }
}
