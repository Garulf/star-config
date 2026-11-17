//! The game's defaults per channel, read once and shared by every screen.

use sc_core::defaults::{self, GameData, Unavailable};
use sc_core::install::Channel;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

pub type Loaded = Arc<Result<GameData, Unavailable>>;

struct Entry {
    build: Option<String>,
    loaded: Loaded,
}

/// One channel's state. `turn` makes loads of one channel take turns; `generation` lets a
/// `forget` that lands during a load keep that load's result out of the store.
#[derive(Default)]
struct Slot {
    turn: Mutex<()>,
    generation: AtomicU64,
    entry: Mutex<Option<Entry>>,
}

impl Slot {
    fn current(&self, build: &Option<String>) -> Option<Loaded> {
        lock(&self.entry)
            .as_ref()
            .filter(|entry| entry.build == *build)
            .map(|entry| entry.loaded.clone())
    }
}

/// Clones share one cache. A result is current while the channel's build id is the one it
/// was read for; a failure is kept too, so a missing `Data.p4k` is not retried on every visit.
#[derive(Clone, Default)]
pub struct GameDataStore {
    slots: Arc<Mutex<HashMap<PathBuf, Arc<Slot>>>>,
}

impl GameDataStore {
    /// Reads the channel's build manifest, so call it from `reload` or background work, not `view`.
    pub fn cached(&self, channel: &Channel) -> Option<Loaded> {
        let slot = lock(&self.slots).get(&channel.dir).cloned()?;
        slot.current(&channel.build_id())
    }

    /// Blocking: reads the cache or the archive. Call it inside `work::background`.
    pub fn load(&self, channel: &Channel, cache_root: &Path) -> Loaded {
        self.load_with(channel, || defaults::load(channel, cache_root))
    }

    fn load_with(
        &self,
        channel: &Channel,
        read: impl FnOnce() -> Result<GameData, Unavailable>,
    ) -> Loaded {
        let slot = self.slot(channel);
        let _turn = lock(&slot.turn);
        let build = channel.build_id();
        if let Some(current) = slot.current(&build) {
            return current;
        }
        let generation = slot.generation.load(Ordering::SeqCst);
        let loaded = Arc::new(read());
        let mut entry = lock(&slot.entry);
        if slot.generation.load(Ordering::SeqCst) == generation {
            *entry = Some(Entry {
                build,
                loaded: loaded.clone(),
            });
        }
        loaded
    }

    /// For a game update noticed while the build id did not change, or a `Data.p4k` that has
    /// appeared since a failed load. A load running now keeps its result to itself.
    pub fn forget(&self, channel: &Channel) {
        if let Some(slot) = lock(&self.slots).get(&channel.dir).cloned() {
            slot.generation.fetch_add(1, Ordering::SeqCst);
            lock(&slot.entry).take();
        }
    }

    /// After an undo or restore, which may have put back any channel's `global.ini`.
    pub fn forget_all(&self) {
        for slot in lock(&self.slots).values() {
            slot.generation.fetch_add(1, Ordering::SeqCst);
            lock(&slot.entry).take();
        }
    }

    fn slot(&self, channel: &Channel) -> Arc<Slot> {
        lock(&self.slots)
            .entry(channel.dir.clone())
            .or_default()
            .clone()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;
    use std::fs;
    use std::sync::Arc;

    fn demo(temp: &std::path::Path) -> crate::context::Context {
        let ctx = testkit::demo_base(temp);
        testkit::demo_game_files(&ctx);
        testkit::seed_game_data(&ctx);
        ctx
    }

    fn live(ctx: &crate::context::Context) -> Channel {
        testkit::channel_named(ctx, "LIVE")
    }

    #[test]
    fn store_loads_once_per_channel() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = demo(temp.path());
        let store = ctx.game_data();
        let live = live(&ctx);
        assert!(store.cached(&live).is_none());

        let first = store.load(&live, &ctx.dirs.game_data());
        let data = first.as_ref().as_ref().expect("fixture cache loads");
        assert_eq!(data.build, live.build_id().unwrap());
        assert_eq!(data.maps.len(), 3);

        fs::remove_dir_all(ctx.dirs.game_data()).unwrap();
        let second = store.load(&live, &ctx.dirs.game_data());
        assert!(Arc::ptr_eq(&first, &second), "the second call reuses it");
        assert!(Arc::ptr_eq(&first, &store.cached(&live).unwrap()));

        let ptu = testkit::channel_named(&ctx, "PTU");
        assert!(store.cached(&ptu).is_none(), "channels are separate");
    }

    #[test]
    fn clones_share_one_store() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = demo(temp.path());
        let live = live(&ctx);
        let loaded = ctx.game_data().load(&live, &ctx.dirs.game_data());
        let again = ctx.clone().game_data().cached(&live).unwrap();
        assert!(Arc::ptr_eq(&loaded, &again));
    }

    #[test]
    fn store_reloads_after_build_change() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = demo(temp.path());
        let store = ctx.game_data();
        let live = live(&ctx);
        let before = store.load(&live, &ctx.dirs.game_data());

        testkit::write_build(&live, testkit::LIVE_VERSION, "9999999");
        assert!(
            store.cached(&live).is_none(),
            "a patched channel has nothing current cached"
        );
        testkit::seed_game_data(&ctx);
        let after = store.load(&live, &ctx.dirs.game_data());
        assert!(!Arc::ptr_eq(&before, &after));
        assert_eq!(
            after.as_ref().as_ref().unwrap().build,
            live.build_id().unwrap()
        );
        assert_ne!(
            after.as_ref().as_ref().unwrap().build,
            before.as_ref().as_ref().unwrap().build
        );
    }

    #[test]
    fn loads_of_one_channel_take_turns() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = demo(temp.path());
        let store = ctx.game_data();
        let live = live(&ctx);
        let reads = AtomicU64::new(0);
        let results: Vec<Loaded> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        store.load_with(&live, || {
                            reads.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(20));
                            defaults::load(&live, &ctx.dirs.game_data())
                        })
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(results.iter().all(|r| Arc::ptr_eq(r, &results[0])));
    }

    #[test]
    fn forget_during_a_load_wins() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = demo(temp.path());
        let store = ctx.game_data();
        let live = live(&ctx);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (store_ref, live_ref, cache) = (&store, &live, ctx.dirs.game_data());
        std::thread::scope(|scope| {
            let loading = scope.spawn(move || {
                store_ref.load_with(live_ref, move || {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    defaults::load(live_ref, &cache)
                })
            });
            started_rx.recv().unwrap();
            store.forget(&live);
            release_tx.send(()).unwrap();
            assert!(
                loading.join().unwrap().is_ok(),
                "the caller still gets its result"
            );
        });
        assert!(store.cached(&live).is_none());
    }

    #[test]
    fn forget_all_clears_every_channel() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = demo(temp.path());
        let store = ctx.game_data();
        let live = live(&ctx);
        let _ = store.load(&live, &ctx.dirs.game_data());
        assert!(store.cached(&live).is_some());
        store.forget_all();
        assert!(store.cached(&live).is_none());
    }

    #[test]
    fn unavailable_is_cached_too() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = testkit::demo_base(temp.path());
        let store = ctx.game_data();
        let live = live(&ctx);

        let first = store.load(&live, &ctx.dirs.game_data());
        assert_eq!(
            first.as_ref().as_ref().unwrap_err(),
            &Unavailable::NoArchive
        );

        testkit::demo_game_files(&ctx);
        testkit::seed_game_data(&ctx);
        let second = store.load(&live, &ctx.dirs.game_data());
        assert!(Arc::ptr_eq(&first, &second), "the failure is not retried");

        store.forget(&live);
        assert!(store.cached(&live).is_none());
        let third = store.load(&live, &ctx.dirs.game_data());
        assert!(third.is_ok(), "forgetting lets the next load try again");
    }
}
