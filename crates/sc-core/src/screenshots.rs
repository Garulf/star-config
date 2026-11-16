//! Screenshot browsing, thumbnails, archive and share copies.
use crate::error::{Error, Result};
use crate::install::Channel;
use crate::safe_write::write_atomic;
use crate::settings::ArchiveMode;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits, RgbImage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const MAX_EDGE: u32 = 16_384;
const MAX_ALLOC: u64 = 1024 * 1024 * 1024;
const MIN_SHARE_EDGE: u32 = 320;
const SHARE_QUALITIES: [u8; 6] = [90, 80, 70, 60, 50, 40];
const THUMBNAIL_QUALITY: u8 = 80;
const SHARED_FOLDER: &str = "shared";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShotKind {
    Image,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    pub path: PathBuf,
    pub channel: Option<String>,
    pub size: u64,
    pub modified_unix: i64,
    pub kind: ShotKind,
}

fn kind_of(path: &Path) -> ShotKind {
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase());
    match extension.as_deref() {
        Some("jpg" | "jpeg" | "png") => ShotKind::Image,
        _ => ShotKind::Other,
    }
}

fn unix_seconds(time: std::time::SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

fn scan_dir(dir: &Path, channel: Option<&str>, out: &mut Vec<Shot>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let path = entry.path();
        out.push(Shot {
            kind: kind_of(&path),
            path,
            channel: channel.map(str::to_string),
            size: meta.len(),
            modified_unix: meta.modified().map_or(0, unix_seconds),
        });
    }
}

pub fn scan(channels: &[Channel], archive_dir: Option<&Path>) -> Vec<Shot> {
    let mut shots = Vec::new();
    for channel in channels {
        scan_dir(&channel.screenshots_dir(), Some(&channel.name), &mut shots);
    }
    if let Some(dir) = archive_dir {
        scan_dir(dir, None, &mut shots);
    }
    shots.sort_by(|a, b| {
        b.modified_unix
            .cmp(&a.modified_unix)
            .then_with(|| a.path.cmp(&b.path))
    });
    shots
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub favourites: BTreeSet<PathBuf>,
    pub origin: BTreeMap<PathBuf, String>,
}

impl Index {
    pub fn load(path: &Path) -> Index {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|err| Error::Invalid(format!("screenshot index: {err}")))?;
        write_atomic(path, &bytes)
    }

    pub fn renamed(&mut self, from: &Path, to: &Path) {
        if self.favourites.remove(from) {
            self.favourites.insert(to.to_path_buf());
        }
        if let Some(channel) = self.origin.remove(from) {
            self.origin.insert(to.to_path_buf(), channel);
        }
    }

    pub fn removed(&mut self, path: &Path) {
        self.favourites.remove(path);
        self.origin.remove(path);
    }
}

fn invalid_image(path: &Path, err: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("{}: {err}", path.display()))
}

fn open_reader(path: &Path) -> Result<ImageReader<BufReader<fs::File>>> {
    let file = fs::File::open(path).map_err(|err| Error::io(path, err))?;
    ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|err| Error::io(path, err))
}

pub fn dimensions(path: &Path) -> Result<(u32, u32)> {
    open_reader(path)?
        .into_dimensions()
        .map_err(|err| invalid_image(path, err))
}

fn checked_dimensions(path: &Path) -> Result<(u32, u32)> {
    let size = fs::metadata(path)
        .map_err(|err| Error::io(path, err))?
        .len();
    if size == 0 {
        return Err(invalid_image(path, "the file is empty"));
    }
    let (width, height) = dimensions(path)?;
    if width > MAX_EDGE || height > MAX_EDGE {
        return Err(invalid_image(
            path,
            format!("{width}x{height} is larger than {MAX_EDGE} pixels on an edge"),
        ));
    }
    Ok((width, height))
}

fn decode(path: &Path) -> Result<DynamicImage> {
    checked_dimensions(path)?;
    let mut reader = open_reader(path)?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    reader.decode().map_err(|err| invalid_image(path, err))
}

fn flatten_onto_black(image: DynamicImage) -> RgbImage {
    if !image.color().has_alpha() {
        return image.into_rgb8();
    }
    let mut rgba = image.into_rgba8();
    for pixel in rgba.pixels_mut() {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = (u16::from(*channel) * alpha / 255) as u8;
        }
    }
    DynamicImage::ImageRgba8(rgba).into_rgb8()
}

#[derive(Debug, Clone)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn load_rgba(path: &Path) -> Result<Rgba> {
    let image = decode(path)?.into_rgba8();
    Ok(Rgba {
        width: image.width(),
        height: image.height(),
        pixels: image.into_raw(),
    })
}

fn encode_jpeg(image: &RgbImage, quality: u8, path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(image)
        .map_err(|err| invalid_image(path, err))?;
    Ok(bytes)
}

fn thumbnail_name(path: &Path, edge: u32) -> Result<String> {
    let meta = fs::metadata(path).map_err(|err| Error::io(path, err))?;
    let modified = meta.modified().map_err(|err| Error::io(path, err))?;
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(meta.len().to_le_bytes());
    hasher.update(unix_seconds(modified).to_le_bytes());
    hasher.update(
        modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
            .to_le_bytes(),
    );
    let digest: String = hasher
        .finalize()
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(format!("{digest}-{edge}.jpg"))
}

pub fn thumbnail(path: &Path, cache_dir: &Path, edge: u32) -> Result<PathBuf> {
    checked_dimensions(path)?;
    let target = cache_dir.join(thumbnail_name(path, edge)?);
    if target.exists() {
        return Ok(target);
    }
    let image = decode(path)?;
    let fitted = if image.width() > edge || image.height() > edge {
        image.resize(edge, edge, FilterType::Triangle)
    } else {
        image
    };
    let bytes = encode_jpeg(&flatten_onto_black(fitted), THUMBNAIL_QUALITY, path)?;
    write_atomic(&target, &bytes)?;
    Ok(target)
}

const FORBIDDEN_NAME_CHARS: [char; 10] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'];

pub fn rename(path: &Path, new_stem: &str) -> Result<PathBuf> {
    let stem = new_stem.trim();
    if stem.is_empty() || stem == "." || stem == ".." || stem.contains(FORBIDDEN_NAME_CHARS) {
        return Err(Error::Invalid(format!(
            "\"{new_stem}\" is not a valid file name"
        )));
    }
    if fs::symlink_metadata(path).is_err() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    let name = match path.extension() {
        Some(ext) => format!("{stem}.{}", ext.to_string_lossy()),
        None => stem.to_string(),
    };
    let target = path.with_file_name(name);
    if fs::symlink_metadata(&target).is_ok() {
        return Err(Error::Invalid(
            "a file with that name already exists".to_string(),
        ));
    }
    fs::rename(path, &target).map_err(|err| Error::io(path, err))?;
    Ok(target)
}

pub fn trash(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_err() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    ::trash::delete(path).map_err(|err| Error::Invalid(format!("{}: {err}", path.display())))
}

fn numbered_path(dir: &Path, stem: &str, extension: Option<&str>) -> PathBuf {
    let build = |suffix: &str| match extension {
        Some(ext) => dir.join(format!("{stem}{suffix}.{ext}")),
        None => dir.join(format!("{stem}{suffix}")),
    };
    let mut candidate = build("");
    let mut number = 2;
    while fs::symlink_metadata(&candidate).is_ok() {
        candidate = build(&format!(" ({number})"));
        number += 1;
    }
    candidate
}

fn is_inside(path: &Path, dir: &Path) -> bool {
    let real = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    real(path).starts_with(real(dir))
}

fn move_across_devices(from: &Path, to: &Path) -> Result<()> {
    let expected = fs::copy(from, to).map_err(|err| Error::io(from, err))?;
    let copied = fs::metadata(to).map_err(|err| Error::io(to, err))?.len();
    if copied != expected {
        let _ = fs::remove_file(to);
        return Err(Error::Invalid(format!(
            "{}: copy is {copied} bytes, expected {expected}",
            to.display()
        )));
    }
    fs::remove_file(from).map_err(|err| Error::io(from, err))
}

pub fn archive(path: &Path, archive_dir: &Path, mode: ArchiveMode) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| Error::Invalid(format!("{} has no file name", path.display())))?;
    if fs::symlink_metadata(path).is_err() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    fs::create_dir_all(archive_dir).map_err(|err| Error::io(archive_dir, err))?;
    if is_inside(path, archive_dir) {
        return Err(Error::Invalid(format!(
            "{} is already in the archive",
            path.display()
        )));
    }
    let name = Path::new(name);
    let stem = name
        .file_stem()
        .unwrap_or(name.as_os_str())
        .to_string_lossy();
    let extension = name.extension().map(|ext| ext.to_string_lossy());
    let target = numbered_path(archive_dir, &stem, extension.as_deref());
    match mode {
        ArchiveMode::Copy => {
            fs::copy(path, &target).map_err(|err| Error::io(path, err))?;
        }
        ArchiveMode::Move => match fs::rename(path, &target) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::CrossesDevices => {
                move_across_devices(path, &target)?;
            }
            Err(err) => return Err(Error::io(path, err)),
        },
    }
    Ok(target)
}

#[derive(Debug, Clone)]
pub struct ShareCopy {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

fn fit_edge(image: DynamicImage, max_edge: u32) -> DynamicImage {
    if image.width() > max_edge || image.height() > max_edge {
        image.resize(max_edge, max_edge, FilterType::Lanczos3)
    } else {
        image
    }
}

fn halved(image: &RgbImage) -> RgbImage {
    let width = (image.width() / 2).max(1);
    let height = (image.height() / 2).max(1);
    image::imageops::resize(image, width, height, FilterType::Lanczos3)
}

fn unchanged_jpeg(
    path: &Path,
    size: (u32, u32),
    max_bytes: u64,
    max_edge: u32,
) -> Option<ShareCopy> {
    let is_jpeg = open_reader(path).ok()?.format() == Some(ImageFormat::Jpeg);
    let bytes = fs::read(path).ok()?;
    let fits = size.0 <= max_edge && size.1 <= max_edge && bytes.len() as u64 <= max_bytes;
    (is_jpeg && fits).then_some(ShareCopy {
        jpeg: bytes,
        width: size.0,
        height: size.1,
    })
}

pub fn share_copy(path: &Path, max_bytes: u64, max_edge: u32) -> Result<ShareCopy> {
    let size = checked_dimensions(path)?;
    if let Some(copy) = unchanged_jpeg(path, size, max_bytes, max_edge) {
        return Ok(copy);
    }
    let mut image = flatten_onto_black(fit_edge(decode(path)?, max_edge));
    loop {
        for quality in SHARE_QUALITIES {
            let jpeg = encode_jpeg(&image, quality, path)?;
            if jpeg.len() as u64 <= max_bytes {
                return Ok(ShareCopy {
                    jpeg,
                    width: image.width(),
                    height: image.height(),
                });
            }
        }
        if image.width().max(image.height()) / 2 < MIN_SHARE_EDGE {
            return Err(Error::Invalid(format!(
                "{} cannot be shrunk to {max_bytes} bytes without dropping under {MIN_SHARE_EDGE} pixels",
                path.display()
            )));
        }
        image = halved(&image);
    }
}

pub fn save_share_copy(
    path: &Path,
    archive_dir: &Path,
    max_bytes: u64,
    max_edge: u32,
) -> Result<PathBuf> {
    let copy = share_copy(path, max_bytes, max_edge)?;
    let stem = path
        .file_stem()
        .map_or_else(|| "screenshot".into(), |s| s.to_string_lossy());
    let dir = archive_dir.join(SHARED_FOLDER);
    fs::create_dir_all(&dir).map_err(|err| Error::io(&dir, err))?;
    let target = numbered_path(&dir, &format!("{stem}-share"), Some("jpg"));
    write_atomic(&target, &copy.jpeg)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::Channel;
    use crate::settings::ArchiveMode;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::tempdir;

    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u8 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 56) as u8
        }
    }

    fn gradient(w: u32, h: u32, noise: u8) -> image::RgbImage {
        let mut rng = Lcg(7);
        image::RgbImage::from_fn(w, h, |x, y| {
            let jitter = |rng: &mut Lcg| {
                if noise == 0 {
                    0
                } else {
                    (rng.next() % noise) as i32
                }
            };
            let base = [(x * 255 / w) as i32, (y * 255 / h) as i32, 128];
            let mut px = [0u8; 3];
            for (out, value) in px.iter_mut().zip(base) {
                *out = (value + jitter(&mut rng)).clamp(0, 255) as u8;
            }
            image::Rgb(px)
        })
    }

    fn write_image(dir: &Path, name: &str, w: u32, h: u32, noise: u8) -> PathBuf {
        let path = dir.join(name);
        gradient(w, h, noise).save(&path).unwrap();
        path
    }

    fn set_mtime(path: &Path, unix: u64) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(unix))
            .unwrap();
    }

    fn channel(root: &Path, name: &str, folder: &str) -> Channel {
        let dir = root.join(name);
        fs::create_dir_all(dir.join(folder)).unwrap();
        Channel {
            name: name.to_string(),
            dir,
        }
    }

    #[test]
    fn scan_lists_channels_and_archive_newest_first() {
        let root = tempdir().unwrap();
        let live = channel(root.path(), "LIVE", "ScreenShots");
        let ptu = channel(root.path(), "PTU", "ScreenShots");
        let archive = root.path().join("archive");
        fs::create_dir_all(&archive).unwrap();
        let a = live.screenshots_dir().join("a.jpg");
        let b = ptu.screenshots_dir().join("b.png");
        let c = archive.join("c.jpg");
        let d = live.screenshots_dir().join("d.jpg");
        for (path, mtime) in [(&a, 100), (&b, 300), (&c, 200), (&d, 100)] {
            fs::write(path, b"12345").unwrap();
            set_mtime(path, mtime);
        }
        let shots = scan(&[live, ptu], Some(&archive));
        let listed: Vec<_> = shots
            .iter()
            .map(|s| (s.path.clone(), s.channel.clone(), s.modified_unix))
            .collect();
        assert_eq!(
            listed,
            [
                (b, Some("PTU".to_string()), 300),
                (c, None, 200),
                (a, Some("LIVE".to_string()), 100),
                (d, Some("LIVE".to_string()), 100),
            ]
        );
        assert_eq!(shots[0].size, 5);
    }

    #[test]
    fn scan_classifies_by_extension_without_opening_files() {
        let root = tempdir().unwrap();
        let live = channel(root.path(), "LIVE", "ScreenShots");
        let dir = live.screenshots_dir();
        fs::write(dir.join("x.jpg"), b"").unwrap();
        fs::write(dir.join("y.PNG"), b"not a png").unwrap();
        fs::write(dir.join("z.JpEg"), b"").unwrap();
        fs::write(dir.join("notes.txt"), b"hi").unwrap();
        let kinds: Vec<_> = scan(&[live], None)
            .into_iter()
            .map(|s| {
                (
                    s.path.file_name().unwrap().to_string_lossy().into_owned(),
                    s.kind,
                )
            })
            .collect();
        let kind_of = |name: &str| kinds.iter().find(|(n, _)| n == name).unwrap().1.clone();
        assert_eq!(kind_of("x.jpg"), ShotKind::Image);
        assert_eq!(kind_of("y.PNG"), ShotKind::Image);
        assert_eq!(kind_of("z.JpEg"), ShotKind::Image);
        assert_eq!(kind_of("notes.txt"), ShotKind::Other);
    }

    #[test]
    fn scan_skips_subfolders_symlinks_and_missing_dirs() {
        let root = tempdir().unwrap();
        let live = channel(root.path(), "LIVE", "ScreenShots");
        let missing = Channel {
            name: "PTU".into(),
            dir: root.path().join("PTU"),
        };
        let archive = root.path().join("archive");
        fs::create_dir_all(archive.join("shared")).unwrap();
        fs::write(archive.join("shared/s-share.jpg"), b"x").unwrap();
        fs::create_dir_all(live.screenshots_dir().join("sub")).unwrap();
        fs::write(live.screenshots_dir().join("sub/deep.jpg"), b"x").unwrap();
        fs::write(live.screenshots_dir().join("real.jpg"), b"x").unwrap();
        #[cfg(unix)]
        {
            let target = root.path().join("outside.jpg");
            fs::write(&target, b"x").unwrap();
            std::os::unix::fs::symlink(&target, live.screenshots_dir().join("link.jpg")).unwrap();
            std::os::unix::fs::symlink(root.path(), live.screenshots_dir().join("dirlink"))
                .unwrap();
        }
        let shots = scan(&[live, missing], Some(&archive));
        let names: Vec<_> = shots
            .iter()
            .map(|s| s.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["real.jpg"]);
        assert!(scan(&[], Some(&root.path().join("nope"))).is_empty());
    }

    #[test]
    fn scan_finds_screenshot_folder_regardless_of_case() {
        let root = tempdir().unwrap();
        let live = channel(root.path(), "LIVE", "screenshots");
        fs::write(root.path().join("LIVE/screenshots/a.png"), b"x").unwrap();
        let shots = scan(&[live], None);
        assert_eq!(shots.len(), 1);
        assert!(shots[0].path.ends_with("screenshots/a.png"));
    }

    #[test]
    fn index_missing_or_corrupt_gives_default_and_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sub/index.json");
        assert_eq!(Index::load(&path), Index::default());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{ not json").unwrap();
        assert_eq!(Index::load(&path), Index::default());
        let mut index = Index::default();
        index.favourites.insert("/a/one.jpg".into());
        index.origin.insert("/arch/two.jpg".into(), "PTU".into());
        index.save(&path).unwrap();
        assert_eq!(Index::load(&path), index);
    }

    #[test]
    fn index_renamed_and_removed_keep_entries_consistent() {
        let mut index = Index::default();
        index.favourites.insert("/a/one.jpg".into());
        index.favourites.insert("/a/keep.jpg".into());
        index.origin.insert("/a/one.jpg".into(), "LIVE".into());
        index.renamed(Path::new("/a/one.jpg"), Path::new("/a/two.jpg"));
        assert!(index.favourites.contains(Path::new("/a/two.jpg")));
        assert!(!index.favourites.contains(Path::new("/a/one.jpg")));
        assert_eq!(index.origin.get(Path::new("/a/two.jpg")).unwrap(), "LIVE");
        assert!(!index.origin.contains_key(Path::new("/a/one.jpg")));
        index.renamed(Path::new("/a/unknown.jpg"), Path::new("/a/x.jpg"));
        assert_eq!(index.favourites.len(), 2);
        index.removed(Path::new("/a/two.jpg"));
        assert!(index.origin.is_empty());
        assert_eq!(index.favourites.len(), 1);
        assert!(index.favourites.contains(Path::new("/a/keep.jpg")));
    }

    #[test]
    fn dimensions_reads_header() {
        let dir = tempdir().unwrap();
        let jpg = write_image(dir.path(), "a.jpg", 640, 360, 0);
        let png = write_image(dir.path(), "a.png", 640, 360, 0);
        assert_eq!(dimensions(&jpg).unwrap(), (640, 360));
        assert_eq!(dimensions(&png).unwrap(), (640, 360));
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn huge_png_header(width: u32, height: u32) -> Vec<u8> {
        let mut ihdr = b"IHDR".to_vec();
        ihdr.extend(width.to_be_bytes());
        ihdr.extend(height.to_be_bytes());
        ihdr.extend([8, 2, 0, 0, 0]);
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend(13u32.to_be_bytes());
        bytes.extend(&ihdr);
        bytes.extend(crc32(&ihdr).to_be_bytes());
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(b"IDAT");
        bytes.extend(crc32(b"IDAT").to_be_bytes());
        bytes
    }

    #[test]
    fn zero_byte_and_oversize_images_are_refused() {
        let dir = tempdir().unwrap();
        let empty = dir.path().join("empty.jpg");
        fs::write(&empty, b"").unwrap();
        let huge = dir.path().join("huge.png");
        fs::write(&huge, huge_png_header(20_000, 20_000)).unwrap();
        let cache = dir.path().join("cache");
        for path in [&empty, &huge] {
            assert!(matches!(load_rgba(path), Err(Error::Invalid(_))));
            assert!(matches!(
                thumbnail(path, &cache, 320),
                Err(Error::Invalid(_))
            ));
            assert!(matches!(
                share_copy(path, 1_000_000, 2560),
                Err(Error::Invalid(_))
            ));
        }
        let message = load_rgba(&huge).unwrap_err().to_string();
        assert!(message.contains("20000"), "{message}");
    }

    #[test]
    fn thumbnail_fits_edge_and_is_cached() {
        let dir = tempdir().unwrap();
        let src = write_image(dir.path(), "a.png", 960, 540, 0);
        let cache = dir.path().join("cache");
        let thumb = thumbnail(&src, &cache, 320).unwrap();
        assert!(thumb.starts_with(&cache));
        assert_eq!(dimensions(&thumb).unwrap(), (320, 180));
        let before = fs::metadata(&thumb).unwrap().modified().unwrap();
        let again = thumbnail(&src, &cache, 320).unwrap();
        assert_eq!(again, thumb);
        assert_eq!(fs::metadata(&again).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn thumbnail_cache_key_changes_when_file_changes() {
        let dir = tempdir().unwrap();
        let src = write_image(dir.path(), "a.png", 64, 64, 0);
        let cache = dir.path().join("cache");
        let first = thumbnail(&src, &cache, 32).unwrap();
        set_mtime(&src, 1_000);
        let second = thumbnail(&src, &cache, 32).unwrap();
        assert_ne!(first, second);
        let other_edge = thumbnail(&src, &cache, 16).unwrap();
        assert_ne!(second, other_edge);
    }

    #[test]
    fn rename_keeps_extension_and_refuses_overwrite_and_bad_names() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("a.jpg");
        fs::write(&src, b"a").unwrap();
        fs::write(dir.path().join("taken.jpg"), b"t").unwrap();
        let renamed = rename(&src, "  fresh name ").unwrap();
        assert_eq!(renamed, dir.path().join("fresh name.jpg"));
        assert!(renamed.exists() && !src.exists());
        for bad in [
            "", "   ", ".", "..", "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b",
            "a\0b",
        ] {
            assert!(
                matches!(rename(&renamed, bad), Err(Error::Invalid(_))),
                "{bad:?}"
            );
        }
        let err = rename(&renamed, "taken").unwrap_err();
        assert_eq!(err.to_string(), "a file with that name already exists");
        assert_eq!(fs::read(dir.path().join("taken.jpg")).unwrap(), b"t");
        assert!(renamed.exists());
    }

    #[test]
    fn trash_missing_file_is_not_found() {
        let dir = tempdir().unwrap();
        assert!(matches!(
            trash(&dir.path().join("gone.jpg")),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn archive_move_and_copy() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("archive");
        let moved = dir.path().join("m.jpg");
        let copied = dir.path().join("c.jpg");
        fs::write(&moved, b"move").unwrap();
        fs::write(&copied, b"copy").unwrap();
        let moved_to = super::archive(&moved, &archive, ArchiveMode::Move).unwrap();
        assert_eq!(moved_to, archive.join("m.jpg"));
        assert!(!moved.exists());
        assert_eq!(fs::read(&moved_to).unwrap(), b"move");
        let copied_to = super::archive(&copied, &archive, ArchiveMode::Copy).unwrap();
        assert_eq!(copied_to, archive.join("c.jpg"));
        assert!(copied.exists());
        assert_eq!(fs::read(&copied_to).unwrap(), b"copy");
    }

    #[test]
    fn archive_numbers_name_clashes() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("archive");
        let first = dir.path().join("one/a.jpg");
        let second = dir.path().join("two/a.jpg");
        for path in [&first, &second] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"x").unwrap();
        }
        let a = super::archive(&first, &archive, ArchiveMode::Move).unwrap();
        let b = super::archive(&second, &archive, ArchiveMode::Move).unwrap();
        assert_eq!(a, archive.join("a.jpg"));
        assert_eq!(b, archive.join("a (2).jpg"));
    }

    #[test]
    fn archive_refuses_file_already_in_archive() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("archive");
        fs::create_dir_all(&archive).unwrap();
        let inside = archive.join("a.jpg");
        fs::write(&inside, b"x").unwrap();
        assert!(matches!(
            super::archive(&inside, &archive, ArchiveMode::Move),
            Err(Error::Invalid(_))
        ));
        assert!(inside.exists());
    }

    #[test]
    fn share_copy_returns_small_jpeg_unchanged() {
        let dir = tempdir().unwrap();
        let src = write_image(dir.path(), "a.jpg", 200, 100, 0);
        let copy = share_copy(&src, 1_000_000, 2560).unwrap();
        assert_eq!(copy.jpeg, fs::read(&src).unwrap());
        assert_eq!((copy.width, copy.height), (200, 100));
    }

    #[test]
    fn share_copy_scales_to_max_edge() {
        let dir = tempdir().unwrap();
        let src = write_image(dir.path(), "a.png", 1000, 500, 0);
        let copy = share_copy(&src, 10_000_000, 640).unwrap();
        assert_eq!((copy.width, copy.height), (640, 320));
        let decoded = image::load_from_memory(&copy.jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (640, 320));
    }

    #[test]
    fn share_copy_lowers_quality_then_size_to_fit() {
        let dir = tempdir().unwrap();
        let src = write_image(dir.path(), "a.png", 1000, 1000, 120);
        let loose = share_copy(&src, 10_000_000, 2560).unwrap();
        assert!(loose.jpeg.len() > 100_000, "{}", loose.jpeg.len());
        let tight = share_copy(&src, 100_000, 2560).unwrap();
        assert!(tight.jpeg.len() <= 100_000, "{}", tight.jpeg.len());
        assert!(tight.width < loose.width);
    }

    #[test]
    fn share_copy_gives_up_below_320() {
        let dir = tempdir().unwrap();
        let src = write_image(dir.path(), "a.png", 800, 400, 120);
        assert!(matches!(
            share_copy(&src, 500, 2560),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn save_share_copy_writes_into_shared_folder() {
        let dir = tempdir().unwrap();
        let src = write_image(dir.path(), "pic.png", 100, 50, 0);
        let archive = dir.path().join("archive");
        let first = save_share_copy(&src, &archive, 1_000_000, 2560).unwrap();
        assert_eq!(first, archive.join("shared/pic-share.jpg"));
        assert_eq!(dimensions(&first).unwrap(), (100, 50));
        let second = save_share_copy(&src, &archive, 1_000_000, 2560).unwrap();
        assert_eq!(second, archive.join("shared/pic-share (2).jpg"));
    }
}
