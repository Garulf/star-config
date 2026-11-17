//! The game's defaults per channel, read once and shared by every screen.

use sc_core::defaults::{self, GameData, Unavailable};
use sc_core::install::Channel;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

pub type Loaded = Arc<Result<GameData, Unavailable>>;

struct Entry {
    build: Option<String>,
    loaded: Loaded,
}

/// Clones share one cache. A result is current while the channel's build id is the one it
/// was read for; a failure is kept too, so a missing `Data.p4k` is not retried on every visit.
#[derive(Clone, Default)]
pub struct GameDataStore {
    entries: Arc<Mutex<HashMap<PathBuf, Entry>>>,
}

impl GameDataStore {
    pub fn cached(&self, channel: &Channel) -> Option<Loaded> {
        let build = channel.build_id();
        self.entries()
            .get(&channel.dir)
            .filter(|entry| entry.build == build)
            .map(|entry| entry.loaded.clone())
    }

    /// Blocking: reads the cache or the archive. Call it inside `work::background`.
    pub fn load(&self, channel: &Channel, cache_root: &Path) -> Loaded {
        if let Some(current) = self.cached(channel) {
            return current;
        }
        let build = channel.build_id();
        let loaded = Arc::new(defaults::load(channel, cache_root));
        self.entries().insert(
            channel.dir.clone(),
            Entry {
                build,
                loaded: loaded.clone(),
            },
        );
        loaded
    }

    /// For a game update noticed while the build id did not change, or a `Data.p4k` that has
    /// appeared since a failed load.
    pub fn forget(&self, channel: &Channel) {
        self.entries().remove(&channel.dir);
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<PathBuf, Entry>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
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
