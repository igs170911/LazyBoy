use std::path::PathBuf;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

/// Vite puts built files in `dist/` and source assets in `public/`.
/// `LAZYBOY_WEB_DIR` is sometimes the source tree (`apps/web`) and sometimes
/// the build output (`apps/web/dist` or Docker `/web`).
pub fn resolve_web_roots(configured: &str) -> (PathBuf, Option<PathBuf>) {
    let configured = PathBuf::from(configured);
    let dist = if configured.file_name().is_some_and(|name| name == "dist") {
        configured.clone()
    } else {
        configured.join("dist")
    };
    let primary = if dist.join("index.html").is_file() {
        dist
    } else {
        configured.clone()
    };
    let public = [
        configured.join("public"),
        configured
            .parent()
            .map(|parent| parent.join("public"))
            .unwrap_or_default(),
    ]
    .into_iter()
    .find(|path| path.is_dir());
    (primary, public)
}

pub fn static_router(configured: &str) -> Router {
    let (primary, public) = resolve_web_roots(configured);
    let fallback = public.clone().unwrap_or_else(|| primary.clone());
    let files = ServeDir::new(&primary).fallback(ServeDir::new(fallback));
    let mut router = Router::new().fallback_service(files);
    if let Some(icon) = first_existing(&[
        primary.join("favicon.ico"),
        primary.join("favicon.png"),
        public
            .as_ref()
            .map(|path| path.join("favicon.ico"))
            .unwrap_or_default(),
        public
            .as_ref()
            .map(|path| path.join("favicon.png"))
            .unwrap_or_default(),
    ]) {
        router = router.route_service("/favicon.ico", ServeFile::new(icon));
    }
    router
}

fn first_existing(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|path| path.is_file()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lazyboy-web-static-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn source_tree_falls_back_to_public_assets() {
        let root = scratch("source");
        fs::write(root.join("index.html"), "<!doctype html>").unwrap();
        fs::create_dir_all(root.join("public")).unwrap();
        fs::write(root.join("public/favicon.svg"), "<svg></svg>").unwrap();
        let (primary, public) = resolve_web_roots(root.to_str().unwrap());
        assert_eq!(primary, root);
        assert_eq!(public.as_deref(), Some(root.join("public").as_path()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prefers_vite_dist_when_present() {
        let root = scratch("built");
        fs::write(root.join("index.html"), "source").unwrap();
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(root.join("dist/index.html"), "built").unwrap();
        fs::create_dir_all(root.join("public")).unwrap();
        let (primary, public) = resolve_web_roots(root.to_str().unwrap());
        assert_eq!(primary, root.join("dist"));
        assert_eq!(public.as_deref(), Some(root.join("public").as_path()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dist_dir_uses_sibling_public() {
        let root = scratch("dist-only");
        let dist = root.join("dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("index.html"), "built").unwrap();
        fs::create_dir_all(root.join("public")).unwrap();
        let (primary, public) = resolve_web_roots(dist.to_str().unwrap());
        assert_eq!(primary, dist);
        assert_eq!(public.as_deref(), Some(root.join("public").as_path()));
        let _ = fs::remove_dir_all(root);
    }
}
