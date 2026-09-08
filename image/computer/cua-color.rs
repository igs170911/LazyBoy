//! LazyBoy's display-local cosmetic override for Cua's built-in cursor.
//! Input/session authority is unchanged. The launcher owns the file path.
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

struct ColorCache {
    path: Option<PathBuf>,
    checked: Option<Instant>,
    color: Option<[u8; 4]>,
}

impl ColorCache {
    fn get(&mut self) -> Option<[u8; 4]> {
        if self
            .checked
            .is_some_and(|time| time.elapsed() < Duration::from_millis(100))
        {
            return self.color;
        }
        self.checked = Some(Instant::now());
        self.color = self.path.as_ref().and_then(|path| {
            let mut value = String::new();
            std::fs::File::open(path)
                .ok()?
                .take(16)
                .read_to_string(&mut value)
                .ok()?;
            parse_color(&value)
        });
        self.color
    }
}

pub(crate) fn configured_color() -> Option<[u8; 4]> {
    static CACHE: OnceLock<Mutex<ColorCache>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            Mutex::new(ColorCache {
                path: std::env::var_os("LAZYBOY_CURSOR_COLOR_FILE").map(PathBuf::from),
                checked: None,
                color: None,
            })
        })
        .lock()
        .ok()?
        .get()
}

fn parse_color(value: &str) -> Option<[u8; 4]> {
    let hex = value.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some([
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
        255,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_user_hex_colors_including_black_and_white() {
        assert_eq!(parse_color("#8b5Cf6\n"), Some([139, 92, 246, 255]));
        assert_eq!(parse_color("#000000"), Some([0, 0, 0, 255]));
        assert_eq!(parse_color("#FFFFFF"), Some([255, 255, 255, 255]));
        for value in [
            "red",
            "#FFF",
            "#GG0000",
            "#12345678",
            "#中文",
            "#123456;touch /tmp/x",
        ] {
            assert_eq!(parse_color(value), None);
        }
    }

    #[test]
    fn updates_are_display_local_and_invalid_files_reset_to_fallback() {
        let root = std::env::temp_dir().join(format!("lazyboy-color-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("a");
        let b = root.join("b");
        std::fs::write(&a, "#123456").unwrap();
        std::fs::write(&b, "#ABCDEF").unwrap();
        let mut first = ColorCache {
            path: Some(a.clone()),
            checked: None,
            color: None,
        };
        let mut second = ColorCache {
            path: Some(b),
            checked: None,
            color: None,
        };
        assert_eq!(first.get(), Some([18, 52, 86, 255]));
        assert_eq!(second.get(), Some([171, 205, 239, 255]));
        std::fs::write(&a, "#FF0000").unwrap();
        first.checked = None;
        assert_eq!(first.get(), Some([255, 0, 0, 255]));
        assert_eq!(second.get(), Some([171, 205, 239, 255]));
        std::fs::remove_file(a).unwrap();
        first.checked = None;
        assert_eq!(first.get(), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
