use std::{
    env, fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use knave_config::{Config, DataDirectories};
use knave_ui::UiImage;

use crate::config::Result;

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;

pub struct Import {
    pub name: String,
    pub preview: UiImage,
    png: Vec<u8>,
}

impl Import {
    pub fn open(path: &Path) -> Result<Self> {
        let image = decode(path)?;
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png)?;
        let png = bytes.into_inner();
        if png.len() as u64 > MAX_BYTES {
            return Err("The converted wallpaper exceeds 64 MiB.".into());
        }
        Ok(Self {
            name: path
                .file_name()
                .ok_or("Choose an image file.")?
                .to_string_lossy()
                .into(),
            preview: preview(image)?,
            png,
        })
    }

    pub fn persist(&self) -> Result<PathBuf> {
        let directory = knave_ui::toolkit::theme_file::data_home()?.join("knave/wallpapers");
        fs::create_dir_all(&directory)?;
        let mut file = tempfile::Builder::new()
            .prefix("wallpaper-")
            .suffix(".png")
            .tempfile_in(directory)?;
        file.write_all(&self.png)?;
        file.as_file().sync_all()?;
        Ok(file.keep()?.1)
    }
}

pub fn effective(config: &Config) -> Option<knave_config::EffectiveWallpaper> {
    let mut data = DataDirectories::from_environment();
    let sibling_assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../knave/share");
    if sibling_assets.is_dir() {
        data.0.push(sibling_assets);
    }
    config.effective_wallpaper(&data)
}

pub fn load_preview(path: &Path) -> Result<UiImage> {
    preview(decode(path)?)
}

fn preview(image: DynamicImage) -> Result<UiImage> {
    let rgba = image.thumbnail(1280, 1280).into_rgba8();
    UiImage::from_rgba(rgba.width(), rgba.height(), rgba.into_raw())
        .ok_or_else(|| "Could not create wallpaper preview.".into())
}

fn decode(path: &Path) -> Result<DynamicImage> {
    if !path.is_file() {
        return Err("Choose a regular PNG or JPEG file.".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The image exceeds 64 MiB.".into());
    }
    let reader = || ImageReader::new(Cursor::new(&bytes)).with_guessed_format();
    let (width, height) = reader()?.into_dimensions()?;
    if width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err("The image exceeds 16384 pixels per side or 16 Mi pixels total.".into());
    }
    let mut reader = reader()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}

pub struct Browser {
    pub directory: PathBuf,
    pub input: String,
    pub entries: Vec<PathBuf>,
}

impl Browser {
    pub fn open(directory: PathBuf) -> Result<Self> {
        let mut entries = fs::read_dir(&directory)?
            .filter_map(std::result::Result::ok)
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_dir()
                    || path.extension().is_some_and(|ext| {
                        ["png", "jpg", "jpeg"]
                            .iter()
                            .any(|format| ext.eq_ignore_ascii_case(format))
                    })
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .is_dir()
                .cmp(&left.is_dir())
                .then_with(|| left.cmp(right))
        });
        Ok(Self {
            input: directory.to_string_lossy().into(),
            directory,
            entries,
        })
    }

    pub fn initial_directory() -> PathBuf {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let pictures = home.join("Pictures");
        if pictures.is_dir() { pictures } else { home }
    }

    pub fn entered_path(&self) -> PathBuf {
        let entered = self.input.trim();
        if let Some(relative) = entered.strip_prefix("~/")
            && let Some(home) = env::var_os("HOME")
        {
            return PathBuf::from(home).join(relative);
        }
        self.directory.join(entered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_import_produces_a_shell_compatible_png() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let original = directory.path().join("wallpaper.jpg");
        DynamicImage::new_rgb8(20, 10).save(&original)?;
        let import = Import::open(&original)?;
        assert_eq!(import.name, "wallpaper.jpg");
        let output = directory.path().join("imported.png");
        fs::write(&output, &import.png)?;
        let decoded = decode(&output)?;
        assert_eq!((decoded.width(), decoded.height()), (20, 10));
        assert_eq!(image::guess_format(&import.png)?, ImageFormat::Png);
        Ok(())
    }
}
