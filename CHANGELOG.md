# Changelog

## [0.3.0](https://github.com/Garulf/star-config/compare/v0.2.0...v0.3.0) (2026-11-17)


### Features

* **config:** pick motion blur and other enumerated cvars from a list ([d6cdea5](https://github.com/Garulf/star-config/commit/d6cdea55b068aab316cdf94124f4677283be07f0))

## [0.2.0](https://github.com/Garulf/star-config/compare/v0.1.0...v0.2.0) (2026-11-17)


### Features

* **app:** bind stick and gamepad cells from a real press ([6bca636](https://github.com/Garulf/star-config/commit/6bca63668360f6d7150dd826c741b603291fc991))
* **app:** one capture box for keys, sticks and gamepads ([2a0281f](https://github.com/Garulf/star-config/commit/2a0281f287bd6df7893103661550f7374e0a6ea9))
* **core:** read USB vendor and product IDs from device GUIDs ([42d4601](https://github.com/Garulf/star-config/commit/42d4601c2619d4ec9ecd234842acd18fb3df30d7))
* **input:** add the device listening crate with axis thresholds ([7347a7f](https://github.com/Garulf/star-config/commit/7347a7fa0a9650b36fc20532eaf47abafc6a6197))
* **input:** read gamepads on Windows through XInput ([1512ea9](https://github.com/Garulf/star-config/commit/1512ea9cc2acfd6e0fdcd37f421b70062a890f38))
* **input:** read sticks and gamepads on Linux through evdev ([cddb6a8](https://github.com/Garulf/star-config/commit/cddb6a8c8ca06dbb8120a2ff4635af610e44a252))
* **input:** read sticks on Windows through DirectInput ([b90fc52](https://github.com/Garulf/star-config/commit/b90fc5296a1056c0ee46a1b5fd05bb59bd8be9bb))


### Bug Fixes

* **app:** name unreadable devices by class and bind only what staged ([7c95831](https://github.com/Garulf/star-config/commit/7c95831423c1a05b891a546dd419c59926e82d6e))
* **app:** tie device messages to their listen and name unreadable devices per platform ([b1e99a8](https://github.com/Garulf/star-config/commit/b1e99a875fb742e5db1831aff848c3ee893cc161))
* **input:** bind centred axes from a quarter-range move ([eb93cf0](https://github.com/Garulf/star-config/commit/eb93cf0e263a68465040013471fc97b805a157b5))
* **input:** keep axis threshold math inside i64 ([acc7430](https://github.com/Garulf/star-config/commit/acc7430441a8f0d3e88de8d4c70c5778d4dec940))
* **input:** mark DirectInput position axes as positions ([8e1ec46](https://github.com/Garulf/star-config/commit/8e1ec46b6c60a8dcb517454419991b0fc1117790))
* **input:** read evdev unsynchronized, report initial axes, reject look-alike devices ([44e4b25](https://github.com/Garulf/star-config/commit/44e4b255aae4d7912fa560cf1848f6641f401d60))

## 0.1.0 (2026-11-17)


### Features

* **app:** add backups screen and recovery of interrupted changes ([b6d730c](https://github.com/Garulf/star-config/commit/b6d730c992d3d806998bb6a0fac835f45d888828))
* **app:** add channels screen with game file copy, swap and settings transfer ([a7c489d](https://github.com/Garulf/star-config/commit/a7c489d9e8508e12be07361e567bee9cd7858d79))
* **app:** add config screen for user.cfg and game settings ([9275083](https://github.com/Garulf/star-config/commit/9275083bf7d2522937e3dc69e85302992fba60b6))
* **app:** add head tracking and comms screen ([a29a985](https://github.com/Garulf/star-config/commit/a29a985d6d00bf705d4fc37907966f4cbea0cd2a))
* **app:** add keybinds screen with capture, conflicts and joystick swap ([0d03670](https://github.com/Garulf/star-config/commit/0d03670b51a19b7cc93af67e56443fa3bafff288))
* **app:** add maintenance screen for shader caches ([379e467](https://github.com/Garulf/star-config/commit/379e46748d1dd825f3307b7bcfc9413bcee8410a))
* **app:** add overview screen ([22027df](https://github.com/Garulf/star-config/commit/22027df1f40e6b2eea73793149321c39e56f7ac1))
* **app:** add screenshots screen with viewer and share actions ([2ff48de](https://github.com/Garulf/star-config/commit/2ff48de6446d83b65825d7e03cab6f7657350878))
* **app:** add StarStrings screen with install, update and uninstall ([69025d1](https://github.com/Garulf/star-config/commit/69025d1ba9250f6025d644a1e2e55d85cf974383))
* **app:** add themed shell with navigation, pickers and launch button ([1c452a8](https://github.com/Garulf/star-config/commit/1c452a86d5703c68a8826a76aa966f6af85ad96e))
* **app:** edit advanced control options like the game's menu ([605fb0d](https://github.com/Garulf/star-config/commit/605fb0d18d55b168bfd9f3e388086ff0c18cdaf5))
* **app:** list every game setting with in-game names and groups ([393cf6b](https://github.com/Garulf/star-config/commit/393cf6b9b75d45104a799ae2ff273818d53e5942))
* **app:** load game data once per channel ([3bb1a91](https://github.com/Garulf/star-config/commit/3bb1a918bcbd26c258bff26be089a0f40c17dee0))
* **app:** show every game action in Keybinds with in-game names ([e6adc9a](https://github.com/Garulf/star-config/commit/e6adc9a9b9e3e6f085855c5b7bf6c666273f4717))
* **core:** add atomic writes, snapshots, guarded writer and staging ([1494361](https://github.com/Garulf/star-config/commit/1494361c2ffd39e123f8dffb18ebba937b151044))
* **core:** add attributes.xml document, catalogue and xml span editor ([55abd2a](https://github.com/Garulf/star-config/commit/55abd2a5439ce789c6d801195ddf0335b5b3e4e7))
* **core:** add channel copy, journaled swap and settings transfer ([df21fa1](https://github.com/Garulf/star-config/commit/df21fa1ef02723d670aaf32d2b86e26a44085aab))
* **core:** add install discovery and case-insensitive path resolution ([b0a03f4](https://github.com/Garulf/star-config/commit/b0a03f4826946ff697cd84a6d46b5a4d0a8b7e94))
* **core:** add keybinds document with rebind, reset and joystick swap ([a00655a](https://github.com/Garulf/star-config/commit/a00655ae2f9e4e3660156267747858170194bf44))
* **core:** add screenshot scanning, thumbnails, archive and share copies ([60fc6a9](https://github.com/Garulf/star-config/commit/60fc6a9a96b0c5132924ebf2e51e3967b4302d4c))
* **core:** add settings file and launcher start ([433e707](https://github.com/Garulf/star-config/commit/433e70736d42f88e690200d3606ebf525c48b8bd))
* **core:** add shader cache discovery, clear, backup and restore ([ee55a2a](https://github.com/Garulf/star-config/commit/ee55a2a946848d8c2f0d86d2856baf9c92eb4c48))
* **core:** add snapshot diffs and the managed file list ([aa8df43](https://github.com/Garulf/star-config/commit/aa8df437a7b36a63d5e736af2cfd8ff8679c4522))
* **core:** add StarStrings check, download validation, install and uninstall ([74cb24f](https://github.com/Garulf/star-config/commit/74cb24f24a780587f3a268cd3a9b98a1ef8faf0f))
* **core:** add the options layout catalogue and its draft script ([e5ab106](https://github.com/Garulf/star-config/commit/e5ab106ecbd76c5733bdbd4cd584f1cd5116efb8))
* **core:** add user.cfg document and cvar catalogue ([c0cf8e8](https://github.com/Garulf/star-config/commit/c0cf8e87cdc0aadeb05a64c919c7ca238bccb976))
* **core:** bind per device kind and write explicit unbinds ([36d49e1](https://github.com/Garulf/star-config/commit/36d49e143e3f5ccd8594e2cc87f86e3c1d4eeb0c))
* **core:** load game defaults from Data.p4k with a per-build cache ([f5745e2](https://github.com/Garulf/star-config/commit/f5745e265299df02268471aab27e90aac746fcec))
* **core:** merge default and user binds and flag only user conflicts ([849c297](https://github.com/Garulf/star-config/commit/849c29715494223d97c749e48fae684b6d6cfebe))
* **core:** parse the game's default profile, settings and strings ([5d3744f](https://github.com/Garulf/star-config/commit/5d3744f18a0fa9fcb8cc052a2372c3c3b27a2899))
* **core:** read and edit advanced control options ([1bcdacb](https://github.com/Garulf/star-config/commit/1bcdacbaa9d0d609bf537e74f074523ae4171beb))
* **core:** read entries and CryXmlB from Data.p4k ([c4e2fe0](https://github.com/Garulf/star-config/commit/c4e2fe0dbcecdb922d034b236c26ed2a20eea5b9))
* first version of star-config ([220b6e6](https://github.com/Garulf/star-config/commit/220b6e6e3ad4bcfd814c06601b604edf0f4afc9c))
* **nix:** add a flake and package so the app installs on NixOS ([b8dcb7c](https://github.com/Garulf/star-config/commit/b8dcb7ce848df6bfb4a61fbba6d73becd2f0858c))


### Bug Fixes

* **app:** address toasts and dialogs by id and queue them ([66017a5](https://github.com/Garulf/star-config/commit/66017a560e517802122305d47bbe72e8dc8080bd))
* **app:** apply background results to the channel they were started in ([eaf14d6](https://github.com/Garulf/star-config/commit/eaf14d6344ede79ccd45cfaade93104f1b0f6114))
* **app:** ask before launching or switching install with unsaved edits ([e286180](https://github.com/Garulf/star-config/commit/e28618018bade862f6d241d5363db8337332d169))
* **app:** check mouse inputs against mouse names only ([c464a7d](https://github.com/Garulf/star-config/commit/c464a7db5ebf1b04a78278071c92993f042daa76))
* **app:** drop the full stop Windows puts on system error texts ([782ea4f](https://github.com/Garulf/star-config/commit/782ea4f7f818bd24ed7e60b86bcebe7026038d3c))
* **app:** give the mouse column room for its button names ([612c667](https://github.com/Garulf/star-config/commit/612c667205cd3c235dd4d85f2615670f0891fe62))
* **app:** keep channel jobs from overlapping ([96d6ec5](https://github.com/Garulf/star-config/commit/96d6ec560459c13766044e3bfe156c851bd32a3b))
* **app:** keep known head tracking defaults on Comms without game data ([c13efb9](https://github.com/Garulf/star-config/commit/c13efb9ac39195bd7d70f22de7171d19114e1e61))
* **app:** keep long names and keyboard binds inside their columns ([2a9c367](https://github.com/Garulf/star-config/commit/2a9c367dd6f32be9c9dc9a1bafade3388083c72f))
* **app:** keep the selected bind and the backup diff scrolled into view ([f4509b5](https://github.com/Garulf/star-config/commit/f4509b5dc7a93d97558ca52458b635df451ff5ba))
* **app:** keep typed advanced values and name every device ([75edf4d](https://github.com/Garulf/star-config/commit/75edf4d8a4fe506e9edb36229b4154480e073cde))
* **app:** load game data for a channel one at a time and honour a forget mid-load ([8c52b64](https://github.com/Garulf/star-config/commit/8c52b64aae6e8493857cff7d17219b35c4cb52d1))
* **app:** never write over a file that changed without telling the user ([1ff892f](https://github.com/Garulf/star-config/commit/1ff892fb14df07854c6f6b83b50a0eb2f7605e44))
* **app:** read game data again after StarStrings changes ([faee5ee](https://github.com/Garulf/star-config/commit/faee5ee30e152e710d2eb01761c410d9cc1891bd))
* **app:** read game names again after an undo or restore ([7036424](https://github.com/Garulf/star-config/commit/7036424ec581219f63b8268e903381969a3ff686))
* **app:** remove a comms value set to its default, as the game does ([553b9f0](https://github.com/Garulf/star-config/commit/553b9f01c3db9fc4b8f1bc84a96672f219532c0f))
* **app:** restore through the guarded writer and stop on unreadable settings ([0714945](https://github.com/Garulf/star-config/commit/071494594fc800ed4ba90a1622ebaa5d43cad540))
* **app:** say what failed in plain words ([d7040a7](https://github.com/Garulf/star-config/commit/d7040a75704a3c0f02aa756191c1c88785ea38ec))
* **app:** set the window's application id so desktop entries match it ([f8d022d](https://github.com/Garulf/star-config/commit/f8d022d34d601e56477ec6603196216d63940931))
* **app:** show the keybinds file first and let missing game data be read again ([eb85847](https://github.com/Garulf/star-config/commit/eb858472f3069fe6a827b71cc0d5a88906f19a27))
* **core:** accept only plain decimals in option values ([a6ce3d1](https://github.com/Garulf/star-config/commit/a6ce3d1436393ec7d6ef0cde909c29c1d4c0d79e))
* **core:** avoid lossy user.cfg edits and settings resets ([2c59418](https://github.com/Garulf/star-config/commit/2c594185b5181350b9353dbda16773b83a32596d))
* **core:** bound CryXmlB output per attribute and accept empty leaf ranges ([2e60bd2](https://github.com/Garulf/star-config/commit/2e60bd28f3f444c526f2b4d1155fb4d2d93c4294))
* **core:** build the read-only test helper on Unix only ([1bcd937](https://github.com/Garulf/star-config/commit/1bcd9370fea2e83e9a91d1017e7794221e98e172))
* **core:** give each atomic write its own temp file ([97f417d](https://github.com/Garulf/star-config/commit/97f417db7a9b6acf189e35e00cec51729d1f39c2))
* **core:** give each game data cache build its own partial folder ([f7f290f](https://github.com/Garulf/star-config/commit/f7f290fa66506b729622b2c65623fa486d04837a))
* **core:** keep a rebind's other attributes when it is replaced ([8e22a35](https://github.com/Garulf/star-config/commit/8e22a35ce6d75ecf154fbe0063f016d4e1a2bd25))
* **core:** keep mouse option groups in the keyboard slot like the game ([dcae641](https://github.com/Garulf/star-config/commit/dcae6410b167f85bf0824634b94f3bd27def640d))
* **core:** keep the StarStrings record and files consistent ([0fc6583](https://github.com/Garulf/star-config/commit/0fc658392e3ffa94df500faf3e3c35652149909b))
* **core:** key installs by canonical path and probe only fixed disks ([a9ca04e](https://github.com/Garulf/star-config/commit/a9ca04e4fb0ddbc430c85e7288154d3943ec327e))
* **core:** make channel swap resume survive lost renames ([16681bf](https://github.com/Garulf/star-config/commit/16681bfcca496924f20cf99eea4907c920d47497))
* **core:** read defaults stated on the device element ([5ef6b2c](https://github.com/Garulf/star-config/commit/5ef6b2ca2b29959c81b830c91075a60316839ac0))
* **core:** report failed rollbacks and guard restores ([86d9492](https://github.com/Garulf/star-config/commit/86d9492f66f624607e458c9b4dc568d7acaa25d8))
* **core:** retry a file replace that Windows refuses for a moment ([d09982b](https://github.com/Garulf/star-config/commit/d09982b20061930d5b27394ec65c6f760ad121dc))
* **core:** sync cross-volume moves before removing the source ([8f260b6](https://github.com/Garulf/star-config/commit/8f260b6436c388ff31adda248536ec73c8972988))
* **core:** tell swapped folders apart by file index on Windows ([3a1f58f](https://github.com/Garulf/star-config/commit/3a1f58f471df736047d0bef0e7d8bb52eeb0f40d))
* show paths with one kind of separator on Windows ([deb54d8](https://github.com/Garulf/star-config/commit/deb54d8ae6dab0c6e9c20c56308695bc7f9442cf))


### Performance Improvements

* **core:** index only the archive entries the loader reads ([d801cf7](https://github.com/Garulf/star-config/commit/d801cf7a7c0fbcf3862a8df33f79e9ec3e1adcfc))
* **core:** index the file's bindings once when merging rows ([5e3c5e9](https://github.com/Garulf/star-config/commit/5e3c5e9181b3cb7d04d24dafe3efab430848d191))
